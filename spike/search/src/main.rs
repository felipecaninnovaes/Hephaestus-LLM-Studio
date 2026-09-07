//! Spike 3f.0 (ADR-0004) — prova binária do pgvector × sqlx runtime × HNSW 512d.
//! Imprime linhas marcadas `C2 PASS/FAIL …` e `C3 PASS/FAIL …` para a matriz do run-spike.sh.

use pgvector::Vector;
use sha2::{Digest, Sha256};
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, QueryBuilder, Row};
use std::time::Instant;

const DIM: usize = 512;
const N: usize = 10_000;
const CLUSTERS: usize = 100;
const QUERIES: usize = 20;

/// Payload canônico do teste C4c (== literal no run-spike.sh / serve.py).
pub const ALIGNED_PAYLOAD: &[u8] = b"SPIKE-VECTOR-ALIGNED-PAYLOAD-000";

struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
    /// uniforme [0,1)
    fn unit(&mut self) -> f32 {
        ((self.next() & ((1u64 << 53) - 1)) as f64 / (1u64 << 53) as f64) as f32
    }
}

/// Algoritmo do mock (idêntico ao serve.py — o C4c prova byte-paridade).
fn mock_vector(payload: &[u8]) -> Vec<f64> {
    let mut h = Sha256::digest(payload).to_vec();
    let mut vals: Vec<f64> = Vec::with_capacity(DIM);
    while vals.len() < DIM {
        h = Sha256::digest(&h).to_vec();
        for i in 0..4 {
            let q = u64::from_le_bytes(h[i * 8..i * 8 + 8].try_into().unwrap())
                & ((1u64 << 53) - 1);
            vals.push((q as f64 / (1u64 << 53) as f64) * 2.0 - 1.0);
        }
    }
    let n = vals.iter().map(|x| x * x).sum::<f64>().sqrt();
    vals.iter().map(|x| x / n).collect()
}

/// Vetor de cluster `c`: base 0.9 na componente (c % DIM), 0.1 nas demais, ruído ±0.03.
fn cluster_vec(rng: &mut Lcg, c: usize) -> Vec<f32> {
    (0..DIM)
        .map(|j| {
            let base = if j % CLUSTERS == c { 0.9 } else { 0.1 };
            base + (rng.unit() - 0.5) * 0.06
        })
        .collect()
}

fn pct(mut xs: Vec<f64>, p: f64) -> f64 {
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    xs[((xs.len() as f64 * p) as usize).min(xs.len() - 1)]
}

#[tokio::main]
async fn main() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&url)
        .await
        .expect("connect");

    // ---------- C2: round-trip crate pgvector + sqlx runtime ----------
    let c2 = run_c2(&pool).await;
    // ---------- referência do mock para o C4c (paridade Rust × Python) ----------
    // payload idêntico ao do batch32.json gerado no run-spike.sh (literal × 64)
    let mv = mock_vector(&ALIGNED_PAYLOAD.repeat(64));
    std::fs::write(
        "rust-vector.txt",
        mv.iter()
            .map(|x| format!("{:.17e}", x))
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .expect("write rust-vector.txt");
    // ---------- C3: HNSW 10k — build, latência, recall ----------
    let c3 = run_c3(&pool).await;

    println!(
        "SPIKE-VERDICT C2={} C3={}",
        if c2 { "PASS" } else { "FAIL" },
        if c3 { "PASS" } else { "FAIL" }
    );
    if !(c2 && c3) {
        std::process::exit(1);
    }
}

async fn run_c2(pool: &PgPool) -> bool {
    sqlx::query("DROP TABLE IF EXISTS spike_vecs")
        .execute(pool)
        .await
        .expect("drop");
    sqlx::query("CREATE TABLE spike_vecs (id BIGSERIAL PRIMARY KEY, v vector(512) NOT NULL)")
        .execute(pool)
        .await
        .expect("create");
    let mut rng = Lcg(42);
    let mut samples: Vec<Vec<f32>> = Vec::new();
    for i in 0..100 {
        let v = cluster_vec(&mut rng, i % CLUSTERS);
        sqlx::query("INSERT INTO spike_vecs (v) VALUES ($1)")
            .bind(Vector::from(v.clone()))
            .execute(pool)
            .await
            .expect("insert c2");
        samples.push(v);
    }
    // query = 8º vetor inserido (id 8): distância <=> de si mesmo deve ser 0
    let q = samples[7].clone();
    let rows = sqlx::query_as::<_, (i64, f64)>(
        "SELECT id, (v <=> $1)::float8 AS dist FROM spike_vecs ORDER BY v <=> $1 LIMIT 5",
    )
    .bind(Vector::from(q.clone()))
    .fetch_all(pool)
    .await
    .expect("query c2");
    let top1_self = rows[0].0 == 8 && rows[0].1.abs() < 1e-6;
    let monotonic = rows.windows(2).all(|w| w[0].1 <= w[1].1);
    let roundtrip: Vector = sqlx::query("SELECT v FROM spike_vecs WHERE id = 8")
        .fetch_one(pool)
        .await
        .expect("select c2")
        .try_get(0)
        .expect("vector col");
    let rt_exact = roundtrip.as_slice() == q.as_slice();
    println!(
        "C2 {} top1_id={} self_dist={:.2e} monotonic={} roundtrip_exact={}",
        if top1_self && monotonic && rt_exact { "PASS" } else { "FAIL" },
        rows[0].0,
        rows[0].1,
        monotonic,
        rt_exact
    );
    top1_self && monotonic && rt_exact
}

async fn run_c3(pool: &PgPool) -> bool {
    sqlx::query("TRUNCATE spike_vecs RESTART IDENTITY")
        .execute(pool)
        .await
        .expect("truncate");
    let mut rng = Lcg(1337);
    let all: Vec<Vec<f32>> = (0..N).map(|i| cluster_vec(&mut rng, i % CLUSTERS)).collect();
    let t_ins = Instant::now();
    for chunk in all.chunks(1000) {
        let mut qb = QueryBuilder::new("INSERT INTO spike_vecs (v) ");
        qb.push_values(chunk.iter(), |mut b, v| {
            b.push_bind(Vector::from(v.clone()));
        });
        qb.build().execute(pool).await.expect("bulk insert");
    }
    println!("C3 info bulk_insert_10k={:.2}s", t_ins.elapsed().as_secs_f64());
    sqlx::query("ANALYZE spike_vecs").execute(pool).await.unwrap();

    // 20 queries determinísticas: membro k*500 + perturbação ±0.01
    let mut rng2 = Lcg(777);
    let qs: Vec<Vec<f32>> = (0..QUERIES)
        .map(|k| {
            let mut q = all[k * (N / QUERIES)].clone();
            for j in 0..DIM {
                q[j] += (rng2.unit() - 0.5) * 0.02;
            }
            q
        })
        .collect();

    // brute force ANTES do índice (seq scan) — referência de recall
    let mut brute: Vec<Vec<i64>> = Vec::new();
    for q in &qs {
        let rows = sqlx::query_as::<_, (i64,)>(
            "SELECT id FROM spike_vecs ORDER BY v <=> $1 LIMIT 10",
        )
        .bind(Vector::from(q.clone()))
        .fetch_all(pool)
        .await
        .expect("brute");
        brute.push(rows.into_iter().map(|r| r.0).collect());
    }

    let t_idx = Instant::now();
    sqlx::query(
        "CREATE INDEX spike_hnsw ON spike_vecs \
         USING hnsw (v vector_cosine_ops) WITH (m = 16, ef_construction = 64)",
    )
    .execute(pool)
    .await
    .expect("create hnsw");
    let idx_s = t_idx.elapsed().as_secs_f64();
    println!("C3 info hnsw_build={:.2}s (limite 30s)", idx_s);

    // queries com o índice — numa ÚNICA sessão, com seqscan off para o planner usar HNSW
    let mut conn = pool.acquire().await.expect("conn");
    sqlx::query("SET enable_seqscan=off").execute(&mut *conn).await.unwrap();
    sqlx::query("SET hnsw.ef_search=40").execute(&mut *conn).await.unwrap();
    let mut hnsw: Vec<Vec<i64>> = Vec::new();
    let mut lat_ms: Vec<f64> = Vec::new();
    for q in &qs {
        let t = Instant::now();
        let rows = sqlx::query_as::<_, (i64,)>(
            "SELECT id FROM spike_vecs ORDER BY v <=> $1 LIMIT 10",
        )
        .bind(Vector::from(q.clone()))
        .fetch_all(&mut *conn)
        .await
        .expect("hnsw query");
        lat_ms.push(t.elapsed().as_micros() as f64 / 1000.0);
        hnsw.push(rows.into_iter().map(|r| r.0).collect());
    }
    drop(conn);

    let top1_match = brute
        .iter()
        .zip(&hnsw)
        .filter(|(b, h)| b[0] == h[0])
        .count();
    let overlap10 = brute
        .iter()
        .zip(&hnsw)
        .map(|(b, h)| b.iter().filter(|id| h.contains(id)).count() as f64 / 10.0)
        .sum::<f64>()
        / QUERIES as f64;
    let (p50, p95) = (pct(lat_ms.clone(), 0.5), pct(lat_ms, 0.95));

    // o que o planner escolhe SEM seqscan off (achado R2 — comportamento em produção);
    // FORMAT TEXT: EXPLAIN (FORMAT JSON) devolve coluna JSON (decode exigiria feature "json")
    let plan: String =
        sqlx::query("EXPLAIN (COSTS OFF) SELECT id FROM spike_vecs ORDER BY v <=> $1 LIMIT 10")
            .bind(Vector::from(qs[0].clone()))
            .fetch_one(pool)
            .await
            .expect("explain")
            .try_get(0)
            .expect("plan col");
    let planner_uses_hnsw = plan.contains("spike_hnsw");

    let build_ok = idx_s < 30.0;
    let lat_ok = p50 < 50.0;
    let recall_ok = top1_match as f64 / QUERIES as f64 >= 0.90;
    println!(
        "C3 info top1_match={}/{} overlap10={:.2} hnsw_p50={:.3}ms p95={:.3}ms planner_sem_seqscanoff_uses_hnsw={}",
        top1_match, QUERIES, overlap10, p50, p95, planner_uses_hnsw
    );
    println!(
        "C3 {}",
        if build_ok && lat_ok && recall_ok { "PASS" } else { "FAIL" }
    );
    build_ok && lat_ok && recall_ok
}
