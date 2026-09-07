//! Indexação assíncrona de embeddings (ADR-0004 D4, fatia 3f.4).
//!
//! Best-effort, sem fila: a "fila" é a diferença entre `images` e
//! `image_embeddings` (estado derivado). O upload dispara fire-and-forget com
//! `only_ids = Some(stored)`; o rebuild (`POST …/search/index`) varre com
//! `None`. Indexadores concorrentes do mesmo dataset se serializam via
//! advisory lock de sessão (`heph_index:{dataset_id}`).

use uuid::Uuid;

/// Tamanho do lote de commit (D4: "lotes commitam em blocos de 100").
const CHUNK_SIZE: usize = 100;
/// Paralelismo do GET de objetos por chunk.
const GET_CONCURRENCY: usize = 4;

/// Indexa imagens de um dataset: retorna o nº de embeddings escritos.
///
/// - `Some(ids)`: só as imagens listadas ainda sem embedding do modelo ativo.
/// - `None`: rebuild — todas as imagens ativas do dataset sem embedding.
/// Idempotente: sem pendentes, escreve 0. Falha de item (storage) pula o
/// item; falha do embedder aborta a rodada com o parcial já escrito (o
/// próximo gatilho repara — D4/R4). Nada aqui estoura 500: quem chama é
/// fire-and-forget.
pub async fn index_dataset_images(
    state: crate::state::AppState,
    dataset_id: Uuid,
    only_ids: Option<Vec<Uuid>>,
) -> u32 {
    let mut conn = match state.pool.acquire().await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[indexer] dataset {dataset_id}: pool indisponível ({e}) — rodada abortada");
            return 0;
        }
    };
    let lock_key = format!("heph_index:{dataset_id}");
    if sqlx::query("SELECT pg_advisory_lock(hashtext($1))")
        .bind(&lock_key)
        .execute(&mut *conn)
        .await
        .is_err()
    {
        eprintln!("[indexer] dataset {dataset_id}: advisory lock falhou — rodada abortada");
        return 0;
    }
    let wrote = index_inner(&state, dataset_id, only_ids).await;
    let _ = sqlx::query("SELECT pg_advisory_unlock(hashtext($1))")
        .bind(&lock_key)
        .execute(&mut *conn)
        .await;
    wrote
}

async fn index_inner(
    state: &crate::state::AppState,
    dataset_id: Uuid,
    only_ids: Option<Vec<Uuid>>,
) -> u32 {
    if matches!(&only_ids, Some(ids) if ids.is_empty()) {
        return 0;
    }
    // Pendentes: imagens ativas ainda sem embedding do modelo ativo.
    let pending: Vec<(Uuid, String)> = match &only_ids {
        Some(ids) => {
            sqlx::query_as(
                "SELECT i.id, i.object_key FROM images i \
                 WHERE i.id = ANY($1) AND i.dataset_id = $2 AND i.deleted_at IS NULL \
                 AND NOT EXISTS (SELECT 1 FROM image_embeddings e WHERE e.image_id = i.id AND e.model = $3)",
            )
            .bind(ids)
            .bind(dataset_id)
            .bind(&state.embedding_model)
            .fetch_all(&state.pool)
            .await
        }
        None => {
            sqlx::query_as(
                "SELECT i.id, i.object_key FROM images i \
                 WHERE i.dataset_id = $1 AND i.deleted_at IS NULL \
                 AND NOT EXISTS (SELECT 1 FROM image_embeddings e WHERE e.image_id = i.id AND e.model = $2)",
            )
            .bind(dataset_id)
            .bind(&state.embedding_model)
            .fetch_all(&state.pool)
            .await
        }
    }
    .unwrap_or_else(|e| {
        eprintln!("[indexer] dataset {dataset_id}: pendentes falhou ({e}) — rodada abortada");
        Vec::new()
    });
    if pending.is_empty() {
        return 0;
    }

    let mut total: u32 = 0;
    for chunk in pending.chunks(CHUNK_SIZE) {
        // (a) GET dos objetos em paralelo com semáforo 4; erro de item pula.
        let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(GET_CONCURRENCY));
        let mut handles = Vec::with_capacity(chunk.len());
        for (id, object_key) in chunk {
            let sem = sem.clone();
            let storage = state.storage.clone();
            let id = *id;
            let object_key = object_key.clone();
            handles.push(tokio::spawn(async move {
                let _permit = sem.acquire_owned().await;
                let bytes = storage.get(&object_key).await;
                (id, object_key, bytes)
            }));
        }
        let mut batch: Vec<(Uuid, Vec<u8>)> = Vec::with_capacity(chunk.len());
        for h in handles {
            match h.await {
                Ok((id, _object_key, Ok(bytes))) => batch.push((id, bytes)),
                Ok((id, object_key, Err(e))) => {
                    eprintln!("[indexer] dataset {dataset_id}: get falhou ({object_key} imagem {id}: {e}) — item pulado");
                }
                Err(e) => {
                    eprintln!(
                        "[indexer] dataset {dataset_id}: task de get falhou ({e}) — item pulado"
                    );
                }
            }
        }
        if batch.is_empty() {
            continue;
        }
        // (b) Embed do chunk; erro aborta a rodada com o parcial escrito.
        let vectors = match state.embedder.embed_images(&batch).await {
            Ok(v) => v,
            Err(e) => {
                eprintln!("[indexer] dataset {dataset_id}: embed falhou ({e}) — rodada abortada com {total} escritos");
                return total;
            }
        };
        // (c) Upsert numa transação única por chunk.
        let mut tx = match state.pool.begin().await {
            Ok(tx) => tx,
            Err(e) => {
                eprintln!("[indexer] dataset {dataset_id}: begin falhou ({e}) — rodada abortada com {total} escritos");
                return total;
            }
        };
        let mut chunk_wrote: u32 = 0;
        let mut tx_failed = false;
        for (id, vec_f32) in &vectors {
            let r = sqlx::query(
                "INSERT INTO image_embeddings (image_id, dataset_id, model, embedding) \
                 VALUES ($1, $2, $3, $4) \
                 ON CONFLICT (image_id) DO UPDATE SET embedding = EXCLUDED.embedding, model = EXCLUDED.model, dataset_id = EXCLUDED.dataset_id, created_at = now()",
            )
            .bind(id)
            .bind(dataset_id)
            .bind(&state.embedding_model)
            .bind(pgvector::Vector::from(vec_f32.clone()))
            .execute(&mut *tx)
            .await;
            match r {
                Ok(_) => chunk_wrote += 1,
                Err(e) => {
                    eprintln!("[indexer] dataset {dataset_id}: upsert falhou (imagem {id}: {e}) — chunk abortado");
                    tx_failed = true;
                    break;
                }
            }
        }
        if tx_failed {
            let _ = tx.rollback().await;
            return total;
        }
        match tx.commit().await {
            Ok(()) => total += chunk_wrote,
            Err(e) => {
                eprintln!("[indexer] dataset {dataset_id}: commit falhou ({e}) — rodada abortada com {total} escritos");
                return total;
            }
        }
    }
    total
}
