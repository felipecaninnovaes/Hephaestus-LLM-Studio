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
//!
//! Wire: `LineageNode`/`LineageEdge`/`LineageResponse` vêm de `heph-contracts`
//! (camelCase direto) — sem DTO espelho aqui nem no BFF, que repassa a
//! resposta do manager sem remapear.
//!
//! Toda aresta aponta no sentido do fluxo de dados (origem→consumidor):
//! dataset→job (`trains`), job→checkpoint|generation (`produced`),
//! checkpoint→job (`resumed_by` no treino, `used_by` na geração). Nunca
//! aresta direta job→job — a UI deriva o pai pelo caminho job→checkpoint→job.

use chrono::{DateTime, Utc};
use heph_contracts::{LineageEdge, LineageNode, LineageResponse};
use sqlx::{PgPool, Row};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

use crate::error::ManagerError;

const MAX_ANCESTOR_DEPTH: usize = 64;

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
    job_id: Uuid,
    label: String,
    epoch: Option<i32>,
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

fn weights_uuid(params: &serde_json::Value) -> Option<Uuid> {
    params
        .get("weights")
        .and_then(|v| v.as_str())
        .and_then(|s| Uuid::parse_str(s).ok())
}

async fn fetch_job_mini(pool: &PgPool, id: Uuid) -> Result<Option<JobMini>, ManagerError> {
    let row = sqlx::query(
        "SELECT id, model, mode, status, created_at, dataset_id, params FROM jobs WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|e| ManagerError::Internal(format!("lineage fetch job: {e}")))?;
    Ok(row.map(row_to_job_mini))
}

fn row_to_job_mini(r: sqlx::postgres::PgRow) -> JobMini {
    JobMini {
        id: r.get("id"),
        model: r.get("model"),
        mode: r.get("mode"),
        status: r.get("status"),
        created_at: r.get("created_at"),
        dataset_id: r.get("dataset_id"),
        params: r.get("params"),
    }
}

/// Resolve um UUID de `params.weights` para o checkpoint que ele referencia,
/// tentando `models` (pesos registrados) com fallback para `job_artifacts`
/// (checkpoints periódicos) — mesma ordem de `resolve_weights_ref`. Única
/// resolução que é necessariamente sequencial (o próximo passo depende do
/// `job_id` deste): 1 lookup por elo da cadeia de ancestrais.
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
        let job_id: Option<Uuid> = row.get("job_id");
        let Some(job_id) = job_id else {
            return Ok(None); // modelo sem job de origem (ex.: upload manual): sem ancestral.
        };
        return Ok(Some(CheckpointRef {
            id: row.get("id"),
            job_id,
            label: row.get("name"),
            epoch: None,
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
            job_id: row.get("job_id"),
            label: basename(&path).to_string(),
            epoch: parse_epoch_from_path(&path),
        }));
    }
    Ok(None)
}

/// Checkpoints/modelos produzidos por QUALQUER job do conjunto dado, numa
/// query por tabela (`= ANY($1)`) — evita N queries por job no grafo.
async fn fetch_produced_checkpoints_batch(
    pool: &PgPool,
    job_ids: &[Uuid],
) -> Result<Vec<CheckpointRef>, ManagerError> {
    if job_ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    let arts = sqlx::query(
        "SELECT id, job_id, path FROM job_artifacts \
         WHERE job_id = ANY($1) AND kind IN ('checkpoint', 'model')",
    )
    .bind(job_ids)
    .fetch_all(pool)
    .await
    .map_err(|e| ManagerError::Internal(format!("lineage produced artifacts: {e}")))?;
    for row in arts {
        let path: String = row.get("path");
        out.push(CheckpointRef {
            id: row.get("id"),
            job_id: row.get("job_id"),
            label: basename(&path).to_string(),
            epoch: parse_epoch_from_path(&path),
        });
    }
    let models = sqlx::query("SELECT id, name, job_id FROM models WHERE job_id = ANY($1)")
        .bind(job_ids)
        .fetch_all(pool)
        .await
        .map_err(|e| ManagerError::Internal(format!("lineage produced models: {e}")))?;
    for row in models {
        out.push(CheckpointRef {
            id: row.get("id"),
            job_id: row.get("job_id"),
            label: row.get("name"),
            epoch: None,
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
    Ok(rows.into_iter().map(row_to_job_mini).collect())
}

/// Gerações de QUALQUER job do conjunto dado (`job_id = ANY($1)`), numa query.
async fn fetch_generations_batch(
    pool: &PgPool,
    job_ids: &[Uuid],
) -> Result<Vec<(Uuid, Uuid, String, DateTime<Utc>)>, ManagerError> {
    if job_ids.is_empty() {
        return Ok(Vec::new());
    }
    let rows = sqlx::query(
        "SELECT id, job_id, prompt, created_at FROM generations \
         WHERE job_id = ANY($1) AND deleted_at IS NULL ORDER BY created_at",
    )
    .bind(job_ids)
    .fetch_all(pool)
    .await
    .map_err(|e| ManagerError::Internal(format!("lineage generations: {e}")))?;
    Ok(rows
        .into_iter()
        .map(|r| {
            (
                r.get("id"),
                r.get("job_id"),
                r.get("prompt"),
                r.get("created_at"),
            )
        })
        .collect())
}

/// Títulos de QUALQUER dataset do conjunto dado (`id = ANY($1)`), numa query.
/// Dataset apagado (id no conjunto mas sem linha) fica fora do mapa — o
/// chamador omite o nó/aresta correspondente, sem erro (referência quebrada).
async fn fetch_dataset_titles_batch(
    pool: &PgPool,
    dataset_ids: &[Uuid],
) -> Result<HashMap<Uuid, String>, ManagerError> {
    if dataset_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows = sqlx::query("SELECT id, title FROM datasets WHERE id = ANY($1)")
        .bind(dataset_ids)
        .fetch_all(pool)
        .await
        .map_err(|e| ManagerError::Internal(format!("lineage dataset titles: {e}")))?;
    Ok(rows
        .into_iter()
        .map(|r| (r.get("id"), r.get("title")))
        .collect())
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

fn job_node(j: &JobMini) -> LineageNode {
    LineageNode {
        id: job_node_id(j.id),
        kind: "job".to_string(),
        label: format!("{} {}", j.mode, j.model),
        status: Some(j.status.clone()),
        created_at: Some(j.created_at.to_rfc3339()),
        epoch: None,
    }
}

fn checkpoint_node(c: &CheckpointRef) -> LineageNode {
    LineageNode {
        id: checkpoint_node_id(c.id),
        kind: "checkpoint".to_string(),
        label: c.label.clone(),
        status: None,
        created_at: None,
        epoch: c.epoch,
    }
}

/// Aresta checkpoint→job pela retomada/geração (sentido do fluxo de dados: o
/// checkpoint alimenta o job); kind depende do modo do job filho.
fn lineage_edge(checkpoint_id: Uuid, child: &JobMini) -> LineageEdge {
    let kind = if child.mode == "generate" {
        "used_by"
    } else {
        "resumed_by"
    };
    LineageEdge {
        from: checkpoint_node_id(checkpoint_id),
        to: job_node_id(child.id),
        kind: kind.to_string(),
    }
}

/// Builder do grafo: dedupe de nós por id, dedupe de arestas por (from,to,kind).
#[derive(Default)]
struct GraphBuilder {
    nodes: HashMap<String, LineageNode>,
    edges: HashSet<LineageEdge>,
}

impl GraphBuilder {
    fn add_node(&mut self, node: LineageNode) {
        self.nodes.entry(node.id.clone()).or_insert(node);
    }

    fn add_edge(&mut self, edge: LineageEdge) {
        self.edges.insert(edge);
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
    let mut ancestor_jobs: Vec<JobMini> = vec![root];

    // Ancestrais: sobe a cadeia de `params.weights` até a raiz, com guarda de
    // ciclo. Sequencial por natureza (cada elo depende do anterior) — 1-2
    // lookups por elo, não dá para agrupar numa query só.
    let mut visited: HashSet<Uuid> = HashSet::from([job_id]);
    // Múltiplas condições de parada após o primeiro `let-else` (checkpoint
    // resolvido, ciclo, job pai apagado) — não cabe em `while let` simples.
    #[allow(clippy::while_let_loop)]
    loop {
        let Some(weights_id) = weights_uuid(&ancestor_jobs.last().unwrap().params) else {
            break;
        };
        let Some(ckpt) = resolve_checkpoint_ref(pool, weights_id).await? else {
            break; // referência quebrada: para silenciosamente.
        };
        if visited.contains(&ckpt.job_id) || ancestor_jobs.len() >= MAX_ANCESTOR_DEPTH {
            g.add_node(checkpoint_node(&ckpt));
            g.add_edge(lineage_edge(ckpt.id, ancestor_jobs.last().unwrap()));
            break; // ciclo (ou limite de segurança): nunca trava.
        }
        let Some(parent) = fetch_job_mini(pool, ckpt.job_id).await? else {
            g.add_node(checkpoint_node(&ckpt));
            g.add_edge(lineage_edge(ckpt.id, ancestor_jobs.last().unwrap()));
            break; // job pai apagado: checkpoint já fica no grafo, para aqui.
        };
        g.add_node(checkpoint_node(&ckpt));
        g.add_edge(lineage_edge(ckpt.id, ancestor_jobs.last().unwrap()));
        visited.insert(parent.id);
        ancestor_jobs.push(parent);
    }

    // Checkpoints do job raiz (precisamos antes dos descendentes, para filtrar por eles).
    let root_checkpoints = fetch_produced_checkpoints_batch(pool, &[job_id]).await?;
    let root_checkpoint_ids: Vec<Uuid> = root_checkpoints.iter().map(|c| c.id).collect();

    // Descendentes diretos: jobs que usaram um checkpoint do job raiz como `weights`.
    let descendant_jobs = fetch_children_by_weights(pool, &root_checkpoint_ids).await?;
    for child in &descendant_jobs {
        if let Some(wid) = weights_uuid(&child.params) {
            g.add_edge(lineage_edge(wid, child));
        }
    }

    // Batch final: checkpoints/gerações/datasets de TODOS os jobs do grafo
    // numa query por tabela, em vez de uma por job.
    let all_job_ids: Vec<Uuid> = ancestor_jobs
        .iter()
        .chain(descendant_jobs.iter())
        .map(|j| j.id)
        .collect();

    let all_checkpoints = fetch_produced_checkpoints_batch(pool, &all_job_ids).await?;
    for ckpt in &all_checkpoints {
        g.add_node(checkpoint_node(ckpt));
        g.add_edge(LineageEdge {
            from: job_node_id(ckpt.job_id),
            to: checkpoint_node_id(ckpt.id),
            kind: "produced".to_string(),
        });
    }

    let generate_job_ids: Vec<Uuid> = ancestor_jobs
        .iter()
        .chain(descendant_jobs.iter())
        .filter(|j| j.mode == "generate")
        .map(|j| j.id)
        .collect();
    let generations = fetch_generations_batch(pool, &generate_job_ids).await?;
    for (gen_id, owner_job_id, prompt, created_at) in generations {
        let label = if prompt.chars().count() > 60 {
            format!("{}…", prompt.chars().take(60).collect::<String>())
        } else {
            prompt
        };
        g.add_node(LineageNode {
            id: generation_node_id(gen_id),
            kind: "generation".to_string(),
            label,
            status: None,
            created_at: Some(created_at.to_rfc3339()),
            epoch: None,
        });
        g.add_edge(LineageEdge {
            from: job_node_id(owner_job_id),
            to: generation_node_id(gen_id),
            kind: "produced".to_string(),
        });
    }

    let dataset_ids: Vec<Uuid> = ancestor_jobs
        .iter()
        .chain(descendant_jobs.iter())
        .filter_map(|j| j.dataset_id)
        .collect();
    let dataset_titles = fetch_dataset_titles_batch(pool, &dataset_ids).await?;
    for j in ancestor_jobs.iter().chain(descendant_jobs.iter()) {
        if let Some(dataset_id) = j.dataset_id {
            if let Some(title) = dataset_titles.get(&dataset_id) {
                g.add_node(LineageNode {
                    id: dataset_node_id(dataset_id),
                    kind: "dataset".to_string(),
                    label: title.clone(),
                    status: None,
                    created_at: None,
                    epoch: None,
                });
                g.add_edge(LineageEdge {
                    from: dataset_node_id(dataset_id),
                    to: job_node_id(j.id),
                    kind: "trains".to_string(),
                });
            }
            // dataset apagado: omite nó/aresta, sem erro (referência quebrada).
        }
    }

    for j in ancestor_jobs.iter().chain(descendant_jobs.iter()) {
        g.add_node(job_node(j));
    }

    Ok(g.into_response())
}
