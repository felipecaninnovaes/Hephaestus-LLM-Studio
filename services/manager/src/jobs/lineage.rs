//! Linhagem dataset→job→checkpoint→resume→geração (fatia 5b).
//!
//! Calculado no manager porque `jobs`/`job_artifacts` são domínio de
//! execução (REPO_MAP §3) e a cadeia de resume vive inteiramente em
//! `jobs.params` (chave `weights`, UUID de um `job_artifacts`/`models`
//! resolvido por `resolve_weights_ref`). Leitura cross-domain de
//! `datasets`/`models`/`generations` já é o padrão estabelecido (manager
//! lê essas tabelas em `resolve.rs`, `models/service.rs`, `generations/service.rs`
//! — mesmo Postgres único, schema compartilhado).
//!
//! Profundidade: ancestrais completos (cadeia de resumes até a raiz, com
//! guarda de ciclo) + descendentes diretos (jobs que retomaram/geraram a
//! partir de um checkpoint do job consultado, e as gerações desses).

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{PgPool, Row};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

use crate::error::ManagerError;

const MAX_ANCESTOR_DEPTH: usize = 64;

#[derive(Debug, Clone, Serialize)]
pub struct LineageNode {
    pub id: String,
    pub kind: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub epoch: Option<i32>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, Hash)]
pub struct LineageEdge {
    pub from: String,
    pub to: String,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct LineageResponse {
    pub nodes: Vec<LineageNode>,
    pub edges: Vec<LineageEdge>,
}

struct JobMini {
    id: Uuid,
    model: String,
    mode: String,
    status: String,
    created_at: DateTime<Utc>,
    dataset_id: Option<Uuid>,
    params: serde_json::Value,
}

/// Checkpoint/modelo resolvido (de `job_artifacts` ou `models`).
struct CheckpointRef {
    id: Uuid,
    label: String,
    epoch: Option<i32>,
    job_id: Option<Uuid>,
}

fn parse_epoch_from_path(path: &str) -> Option<i32> {
    let idx = path.find("epoch_")?;
    let rest = &path[idx + "epoch_".len()..];
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        None
    } else {
        digits.parse().ok()
    }
}

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

async fn fetch_job_mini(pool: &PgPool, id: Uuid) -> Result<Option<JobMini>, ManagerError> {
    let row = sqlx::query(
        "SELECT id, model, mode, status, created_at, dataset_id, params FROM jobs WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|e| ManagerError::Internal(format!("lineage fetch job: {e}")))?;
    Ok(row.map(|r| JobMini {
        id: r.get("id"),
        model: r.get("model"),
        mode: r.get("mode"),
        status: r.get("status"),
        created_at: r.get("created_at"),
        dataset_id: r.get("dataset_id"),
        params: r.get("params"),
    }))
}

/// Resolve um UUID de `params.weights` para o checkpoint que ele referencia,
/// tentando `models` (pesos registrados) com fallback para `job_artifacts`
/// (checkpoints periódicos) — mesma ordem de `resolve_weights_ref`.
async fn resolve_checkpoint_ref(
    pool: &PgPool,
    weights_id: Uuid,
) -> Result<Option<CheckpointRef>, ManagerError> {
    if let Some(row) = sqlx::query("SELECT id, name, job_id FROM models WHERE id = $1")
        .bind(weights_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| ManagerError::Internal(format!("lineage resolve model: {e}")))?
    {
        let name: String = row.get("name");
        return Ok(Some(CheckpointRef {
            id: row.get("id"),
            label: name,
            epoch: None,
            job_id: row.get("job_id"),
        }));
    }
    if let Some(row) = sqlx::query(
        "SELECT id, job_id, path FROM job_artifacts WHERE id = $1 AND kind IN ('checkpoint', 'model')",
    )
    .bind(weights_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| ManagerError::Internal(format!("lineage resolve artifact: {e}")))?
    {
        let path: String = row.get("path");
        return Ok(Some(CheckpointRef {
            id: row.get("id"),
            label: basename(&path).to_string(),
            epoch: parse_epoch_from_path(&path),
            job_id: Some(row.get("job_id")),
        }));
    }
    Ok(None)
}

/// Checkpoints/modelos produzidos por um job (artefatos de treino + registro em `models`).
async fn fetch_produced_checkpoints(
    pool: &PgPool,
    job_id: Uuid,
) -> Result<Vec<CheckpointRef>, ManagerError> {
    let mut out = Vec::new();
    let arts = sqlx::query(
        "SELECT id, path FROM job_artifacts WHERE job_id = $1 AND kind IN ('checkpoint', 'model')",
    )
    .bind(job_id)
    .fetch_all(pool)
    .await
    .map_err(|e| ManagerError::Internal(format!("lineage produced artifacts: {e}")))?;
    for row in arts {
        let path: String = row.get("path");
        out.push(CheckpointRef {
            id: row.get("id"),
            label: basename(&path).to_string(),
            epoch: parse_epoch_from_path(&path),
            job_id: Some(job_id),
        });
    }
    let models = sqlx::query("SELECT id, name FROM models WHERE job_id = $1")
        .bind(job_id)
        .fetch_all(pool)
        .await
        .map_err(|e| ManagerError::Internal(format!("lineage produced models: {e}")))?;
    for row in models {
        out.push(CheckpointRef {
            id: row.get("id"),
            label: row.get("name"),
            epoch: None,
            job_id: Some(job_id),
        });
    }
    Ok(out)
}

/// Jobs cujo `params.weights` resolve para um dos checkpoints dados (descendentes diretos).
async fn fetch_children_by_weights(
    pool: &PgPool,
    checkpoint_ids: &[Uuid],
) -> Result<Vec<JobMini>, ManagerError> {
    if checkpoint_ids.is_empty() {
        return Ok(Vec::new());
    }
    let ids: Vec<String> = checkpoint_ids.iter().map(|u| u.to_string()).collect();
    let rows = sqlx::query(
        "SELECT id, model, mode, status, created_at, dataset_id, params FROM jobs \
         WHERE params->>'weights' = ANY($1)",
    )
    .bind(&ids)
    .fetch_all(pool)
    .await
    .map_err(|e| ManagerError::Internal(format!("lineage children: {e}")))?;
    Ok(rows
        .into_iter()
        .map(|r| JobMini {
            id: r.get("id"),
            model: r.get("model"),
            mode: r.get("mode"),
            status: r.get("status"),
            created_at: r.get("created_at"),
            dataset_id: r.get("dataset_id"),
            params: r.get("params"),
        })
        .collect())
}

async fn fetch_generations_for_job(
    pool: &PgPool,
    job_id: Uuid,
) -> Result<Vec<(Uuid, String, DateTime<Utc>)>, ManagerError> {
    let rows = sqlx::query(
        "SELECT id, prompt, created_at FROM generations \
         WHERE job_id = $1 AND deleted_at IS NULL ORDER BY created_at",
    )
    .bind(job_id)
    .fetch_all(pool)
    .await
    .map_err(|e| ManagerError::Internal(format!("lineage generations: {e}")))?;
    Ok(rows
        .into_iter()
        .map(|r| (r.get("id"), r.get("prompt"), r.get("created_at")))
        .collect())
}

async fn fetch_dataset_title(pool: &PgPool, id: Uuid) -> Result<Option<String>, ManagerError> {
    sqlx::query_scalar("SELECT title FROM datasets WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|e| ManagerError::Internal(format!("lineage dataset title: {e}")))
}

/// Builder incremental do grafo: dedupe de nós por id, dedupe de arestas por (from,to,kind).
#[derive(Default)]
struct GraphBuilder {
    nodes: HashMap<String, LineageNode>,
    edges: HashSet<LineageEdge>,
}

impl GraphBuilder {
    fn add_node(&mut self, node: LineageNode) {
        self.nodes.entry(node.id.clone()).or_insert(node);
    }

    fn add_edge(&mut self, from: String, to: String, kind: &str) {
        self.edges.insert(LineageEdge {
            from,
            to,
            kind: kind.to_string(),
        });
    }

    fn job_node_id(id: Uuid) -> String {
        format!("job:{id}")
    }
    fn checkpoint_node_id(id: Uuid) -> String {
        format!("checkpoint:{id}")
    }
    fn dataset_node_id(id: Uuid) -> String {
        format!("dataset:{id}")
    }
    fn generation_node_id(id: Uuid) -> String {
        format!("generation:{id}")
    }

    fn add_job(&mut self, j: &JobMini) {
        self.add_node(LineageNode {
            id: Self::job_node_id(j.id),
            kind: "job".to_string(),
            label: format!("{} {}", j.mode, j.model),
            status: Some(j.status.clone()),
            created_at: Some(j.created_at.to_rfc3339()),
            epoch: None,
        });
    }

    fn add_checkpoint(&mut self, c: &CheckpointRef) {
        self.add_node(LineageNode {
            id: Self::checkpoint_node_id(c.id),
            kind: "checkpoint".to_string(),
            label: c.label.clone(),
            status: None,
            created_at: None,
            epoch: c.epoch,
        });
    }

    async fn add_dataset_edge(&mut self, pool: &PgPool, j: &JobMini) -> Result<(), ManagerError> {
        let Some(dataset_id) = j.dataset_id else {
            return Ok(());
        };
        let Some(title) = fetch_dataset_title(pool, dataset_id).await? else {
            return Ok(()); // dataset apagado: omite nó, sem erro (relação quebrada).
        };
        self.add_node(LineageNode {
            id: Self::dataset_node_id(dataset_id),
            kind: "dataset".to_string(),
            label: title,
            status: None,
            created_at: None,
            epoch: None,
        });
        self.add_edge(
            Self::dataset_node_id(dataset_id),
            Self::job_node_id(j.id),
            "trains",
        );
        Ok(())
    }

    async fn add_produced_checkpoints(
        &mut self,
        pool: &PgPool,
        job_id: Uuid,
    ) -> Result<(), ManagerError> {
        for ckpt in fetch_produced_checkpoints(pool, job_id).await? {
            self.add_checkpoint(&ckpt);
            self.add_edge(
                Self::job_node_id(job_id),
                Self::checkpoint_node_id(ckpt.id),
                "produced",
            );
        }
        Ok(())
    }

    async fn add_generations(&mut self, pool: &PgPool, j: &JobMini) -> Result<(), ManagerError> {
        if j.mode != "generate" {
            return Ok(());
        }
        for (gen_id, prompt, created_at) in fetch_generations_for_job(pool, j.id).await? {
            let label = if prompt.chars().count() > 60 {
                format!("{}…", prompt.chars().take(60).collect::<String>())
            } else {
                prompt
            };
            self.add_node(LineageNode {
                id: Self::generation_node_id(gen_id),
                kind: "generation".to_string(),
                label,
                status: None,
                created_at: Some(created_at.to_rfc3339()),
                epoch: None,
            });
            self.add_edge(
                Self::job_node_id(j.id),
                Self::generation_node_id(gen_id),
                "produced",
            );
        }
        Ok(())
    }

    /// Aresta checkpoint→job pela retomada/geração (sentido do fluxo de
    /// dados: o checkpoint alimenta o job); kind depende do modo do job filho.
    /// A UI deriva o job pai seguindo job→checkpoint→job (sem aresta direta
    /// job→job, que duplicaria a mesma relação em dois sentidos).
    fn add_lineage_edge(&mut self, checkpoint_id: Uuid, child: &JobMini) {
        let kind = if child.mode == "generate" {
            "used_by"
        } else {
            "resumed_by"
        };
        self.add_edge(
            Self::checkpoint_node_id(checkpoint_id),
            Self::job_node_id(child.id),
            kind,
        );
    }

    fn into_response(self) -> LineageResponse {
        let mut nodes: Vec<LineageNode> = self.nodes.into_values().collect();
        nodes.sort_by(|a, b| a.id.cmp(&b.id));
        let mut edges: Vec<LineageEdge> = self.edges.into_iter().collect();
        edges.sort_by(|a, b| (&a.from, &a.to, &a.kind).cmp(&(&b.from, &b.to, &b.kind)));
        LineageResponse { nodes, edges }
    }
}

/// Calcula a linhagem completa de um job (ancestrais + descendentes diretos).
/// Job inexistente ⇒ `ManagerError::NotFound`. Referência quebrada (pai
/// apagado, dataset apagado) ⇒ nó/aresta correspondente omitido, sem erro.
pub async fn get_job_lineage(pool: &PgPool, job_id: Uuid) -> Result<LineageResponse, ManagerError> {
    let root = fetch_job_mini(pool, job_id)
        .await?
        .ok_or(ManagerError::NotFound)?;

    let mut g = GraphBuilder::default();
    g.add_job(&root);
    g.add_dataset_edge(pool, &root).await?;
    g.add_produced_checkpoints(pool, root.id).await?;
    g.add_generations(pool, &root).await?;

    // Ancestrais: sobe a cadeia de `params.weights` até a raiz, com guarda de ciclo.
    let mut visited: HashSet<Uuid> = HashSet::from([root.id]);
    let mut current = root;
    for _ in 0..MAX_ANCESTOR_DEPTH {
        let weights_id = current
            .params
            .get("weights")
            .and_then(|v| v.as_str())
            .and_then(|s| Uuid::parse_str(s).ok());
        let Some(weights_id) = weights_id else {
            break;
        };
        let Some(ckpt) = resolve_checkpoint_ref(pool, weights_id).await? else {
            break; // referência quebrada: para silenciosamente.
        };
        g.add_checkpoint(&ckpt);
        g.add_lineage_edge(ckpt.id, &current);
        let Some(parent_id) = ckpt.job_id else {
            break; // checkpoint sem job de origem (ex.: upload manual).
        };
        if visited.contains(&parent_id) {
            break; // ciclo: nunca trava.
        }
        let Some(parent) = fetch_job_mini(pool, parent_id).await? else {
            break; // job pai apagado: checkpoint já ficou no grafo, para aqui.
        };
        visited.insert(parent.id);
        g.add_job(&parent);
        g.add_dataset_edge(pool, &parent).await?;
        g.add_produced_checkpoints(pool, parent.id).await?;
        g.add_generations(pool, &parent).await?;
        current = parent;
    }

    // Descendentes diretos: jobs que usaram um checkpoint do job raiz como `weights`.
    let root_checkpoints = fetch_produced_checkpoints(pool, job_id).await?;
    let checkpoint_ids: Vec<Uuid> = root_checkpoints.iter().map(|c| c.id).collect();
    let children = fetch_children_by_weights(pool, &checkpoint_ids).await?;
    for child in children {
        if visited.contains(&child.id) {
            continue; // já coberto pela subida de ancestrais (não deveria ocorrer).
        }
        let weights_id = child
            .params
            .get("weights")
            .and_then(|v| v.as_str())
            .and_then(|s| Uuid::parse_str(s).ok());
        g.add_job(&child);
        if let Some(wid) = weights_id {
            g.add_lineage_edge(wid, &child);
        }
        g.add_dataset_edge(pool, &child).await?;
        g.add_generations(pool, &child).await?;
    }

    Ok(g.into_response())
}
