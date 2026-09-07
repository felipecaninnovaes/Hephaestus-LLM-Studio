#!/usr/bin/env bash
# Spike 3f.0 (ADR-0004) — pgvector × sqlx × HNSW × embedder mock × pg_dump.
# Script ÚNICO: roda todas as provas e imprime a matriz de resultados (lição 3b.0).
# Containers/volumes `spike-*` são temporários e limpos no fim. NÃO toca no compose de dev.
set -uo pipefail

cd "$(dirname "$0")"
RESULTS=spike-results.txt
: > "$RESULTS"
log() { echo "$@" | tee -a "$RESULTS"; }
step() { log "" ; log "== $* =="; }

PGVECTOR="pgvector/pgvector:pg16-trixie@sha256:c8483555ce48101872f888c1df8a895ff689d6c7c7a5f7ac266475f9dfe89e0b"
PGORIG="postgres:16@sha256:f1c3376c26f2609ab9f29f71f824103fe2fcd8ee0346485cb6122a4f93df6f94"
DBPASS=studio
C1=0 C2=0 C3=0 C4=0 C5=0

cleanup() {
  docker rm -f spike-c1 spike-c5 spike-embedder >/dev/null 2>&1
  docker volume rm spike_pgdata_c1 spike_pgdata_c5 >/dev/null 2>&1
  rm -f spike.dump batch32.json rust-vector.txt 2>/dev/null
}
trap cleanup EXIT

wait_ready() { # container, usuário, banco — espera psql real (SELECT 1) por 60s
  for _ in $(seq 1 60); do
    docker exec "$1" psql -U "$2" -d "$3" -tAc "SELECT 1" >/dev/null 2>&1 && return 0
    sleep 1
  done
  return 1
}

# ---------------------------------------------------------------- C1
step "C1: pgvector/pgvector:pg16-trixie sobe com o volume do postgres:16 (sem dump/restore)"
# idempotência: remove resíduos de rodadas anteriores
docker rm -f spike-c1 spike-c5 spike-embedder >/dev/null 2>&1
docker volume rm spike_pgdata_c1 spike_pgdata_c5 >/dev/null 2>&1
docker volume create spike_pgdata_c1 >/dev/null
log "cópia do volume real infra_pgdata -> spike_pgdata_c1 (cp -a, imagem $PGORIG)"
docker run --rm -v infra_pgdata:/from -v spike_pgdata_c1:/to "$PGORIG" cp -a /from/. /to/ | tee -a "$RESULTS"
docker run -d --name spike-c1 -v spike_pgdata_c1:/var/lib/postgresql/data -p 5433:5432 "$PGVECTOR" | tee -a "$RESULTS"
if wait_ready spike-c1 studio studio; then
  # snapshot do log de boot ANTES de qualquer sonda; "FATAL: the database system
  # is starting up" é benigno (volume copiado quente entra em crash recovery do WAL)
  docker logs spike-c1 > spike-c1.log 2>&1
  LOGFAIL=$(grep -iE "fatal|incompatible|could not open" spike-c1.log | grep -vci "the database system is starting up" || true)
  LOGINFO=$(grep -c "database system is ready to accept connections" spike-c1.log || true)
  RECOVERY=$(grep -ci "automatic recovery in progress" spike-c1.log || true)
  EXTOK=$(docker exec spike-c1 psql -U studio -d studio -tAc \
    "CREATE EXTENSION IF NOT EXISTS vector; SELECT count(*) FROM pg_extension WHERE extname='vector';" 2>&1 | tail -1)
  TBL=$(docker exec spike-c1 psql -U studio -d studio -tAc \
    "SELECT count(*) FROM information_schema.tables WHERE table_schema='public';" 2>&1 | tail -1)
  # banco limpo para C2/C3
  docker exec spike-c1 psql -U studio -d studio -c "CREATE DATABASE spikedb;" >/dev/null 2>&1
  docker exec spike-c1 psql -U studio -d spikedb -c "CREATE EXTENSION vector;" >/dev/null 2>&1
  VERS=$(docker exec spike-c1 psql -U studio -d spikedb -tAc "SELECT version();" | grep -oE "PostgreSQL [0-9.]+")
  log "boot: ready=$LOGINFO, fatais reais no log de boot=$LOGFAIL, crash recovery de volume quente=$RECOVERY; CREATE EXTENSION no banco REAL migrado -> count=$EXTOK; tabelas do dev no volume: $TBL; $VERS (spikedb p/ C2/C3)"
  if [ "$EXTOK" = "1" ] && [ "$LOGFAIL" = "0" ] && [ "$TBL" -ge 9 ]; then C1=1; fi
else
  log "container não ficou ready em 60s"; docker logs spike-c1 2>&1 | tail -20 | tee -a "$RESULTS"
fi

# ---------------------------------------------------------------- C2 + C3 (cargo)
if [ "$C1" = "1" ]; then
  step "C2/C3: crate pgvector (sqlx runtime) round-trip + HNSW 10k"
  export DATABASE_URL="postgres://studio:${DBPASS}@localhost:5433/spikedb"
  if cargo run --release 2>build.log | tee -a "$RESULTS"; then
    C2=$(grep -c "^C2 PASS" "$RESULTS")
    C3=$(grep -c "^C3 PASS" "$RESULTS")
  else
    log "cargo run FALHOU:"; tail -30 build.log | tee -a "$RESULTS"
  fi
else
  log "C1 falhou — C2/C3 abortados"
fi

# ---------------------------------------------------------------- C4 (embedder mock)
step "C4: container embedder mock (ENGINE_MOCK=1 — stdlib, sem torch)"
python3 - <<'PY'
import base64, json
items = [{"id": i, "b64": base64.b64encode((f"SPIKE-VECTOR-ALIGNED-PAYLOAD-{i:03d}").encode() * 64).decode()} for i in range(32)]
open("batch32.json", "w").write(json.dumps({"model": "ViT-B-32", "items": items}))
PY
C4_READY=0
if docker build -t spike-embedder mockserve/ >embed-build.log 2>&1; then
  if docker run -d --name spike-embedder -p 18090:8090 spike-embedder >/dev/null; then
    for _ in $(seq 1 30); do
      if curl -sf http://localhost:18090/health >/dev/null 2>&1; then C4_READY=1; break; fi
      sleep 1
    done
  else
    log "docker run spike-embedder falhou:"
    docker logs spike-embedder 2>&1 | tail -10 | tee -a "$RESULTS"
  fi
else
  log "docker build do embedder falhou:"
  tail -15 embed-build.log | tee -a "$RESULTS"
fi
if [ "$C4_READY" = "1" ]; then
  curl -sf http://localhost:18090/health | tee -a "$RESULTS"
  log ""
  T_EMBED=$(curl -s -o /tmp/embed-resp.json -w '%{time_total}' -X POST http://localhost:18090/embed -H 'Content-Type: application/json' -d @batch32.json)
  T_TEXT=$(curl -s -o /tmp/embed-text-resp.json -w '%{time_total}' -X POST http://localhost:18090/embed-text -H 'Content-Type: application/json' -d '{"model":"ViT-B-32","texts":["um gato brincando no sofá","defeito de solda no circuito","rua movimentada em São Paulo"]}')
  N_ITEMS=$(python3 -c "import json;d=json.load(open('/tmp/embed-resp.json'));print(len(d['items']),d['items'][0]['dim'])")
  log "POST /embed  (batch 32, ~2KB/item): ${T_EMBED}s | POST /embed-text (3 textos): ${T_TEXT}s | resposta: $N_ITEMS itens×dim (limite 0.5s cada)"
  # C4c: paridade do mock — vetor canônico computado em Rust, Python puro e no serve.py
  PARITY=$(python3 <<'PY'
import hashlib, json, math, struct

def mock_vector(payload: bytes):
    h = hashlib.sha256(payload).digest()
    vals = []
    while len(vals) < 512:
        h = hashlib.sha256(h).digest()
        for i in range(4):
            q = struct.unpack_from("<Q", h, i * 8)[0] & ((1 << 53) - 1)
            vals.append((q / float(1 << 53)) * 2.0 - 1.0)
    n = math.sqrt(sum(x * x for x in vals))
    return [x / n for x in vals]

payload = b"SPIKE-VECTOR-ALIGNED-PAYLOAD-000" * 64  # idêntico ao do batch32.json (item 0)
rust = [float(x) for x in open("rust-vector.txt")]
serve = json.load(open("/tmp/embed-resp.json"))["items"][0]["vector"]
local = mock_vector(payload)
d_rust = max(abs(a - b) for a, b in zip(rust, local))
d_serve = max(abs(a - b) for a, b in zip(serve, local))
print(f"rust×python maxdiff={d_rust:.2e} serve.py×python maxdiff={d_serve:.2e}")
print("C4c PASS" if d_rust < 1e-9 and d_serve < 1e-9 else "C4c FAIL")
PY
)
  log "$PARITY" | tee -a "$RESULTS"
  T4=$(echo "$PARITY" | grep -oE "maxdiff=[0-9.e+-]+" | head -1 | cut -d= -f2)
  T5=$(echo "$PARITY" | grep -oE "maxdiff=[0-9.e+-]+" | tail -1 | cut -d= -f2)
  if [ "$(echo "$PARITY" | grep -c 'C4c PASS')" = "1" ] && python3 -c "exit(0 if float('$T_EMBED')<0.5 and float('$T_TEXT')<0.5 and float('$T4')<1e-9 and float('$T5')<1e-9 else 1)"; then
    C4=1
  fi
fi

# ---------------------------------------------------------------- C5 (pg_dump/restore)
step "C5: pg_dump com vetores -> restore preserva o tipo"
docker volume create spike_pgdata_c5 >/dev/null
docker run -d --name spike-c5 -v spike_pgdata_c5:/var/lib/postgresql/data \
  -e POSTGRES_USER=spike -e POSTGRES_PASSWORD=spike -e POSTGRES_DB=spike -p 5434:5432 "$PGVECTOR" >/dev/null
if wait_ready spike-c5 spike spike; then
  docker exec spike-c1 pg_dump -U studio -Fc spikedb > spike.dump
  docker cp spike.dump spike-c5:/tmp/spike.dump >/dev/null
  RESTORE_ERR=$(docker exec spike-c5 pg_restore -U spike -d spike --no-owner /tmp/spike.dump 2>&1)
  log "pg_restore: ${RESTORE_ERR:-sem erros}"
  if [ -z "$RESTORE_ERR" ]; then
    # md5(v::text) compara TODOS os 512 valores (subscripting v[i] não existe no pgvector 0.8.6)
    CNT=$(docker exec spike-c5 psql -U spike -d spike -tAc "SELECT count(*) FROM spike_vecs;")
    EXT=$(docker exec spike-c5 psql -U spike -d spike -tAc "SELECT count(*) FROM pg_extension WHERE extname='vector';")
    V1=$(docker exec spike-c1 psql -U studio -d spikedb -tAc "SELECT md5(v::text) FROM spike_vecs WHERE id=1;")
    V5=$(docker exec spike-c5 psql -U spike -d spike -tAc "SELECT md5(v::text) FROM spike_vecs WHERE id=1;")
    SELF=$(docker exec spike-c5 psql -U spike -d spike -tAc "SELECT (v <=> v)::float8 FROM spike_vecs WHERE id=1;")
    log "restaurado: rows=$CNT ext_vector=$EXT md5(v::text) id1=$V5 (origem $V1) v<=>v=$SELF"
    if [ "$CNT" = "10000" ] && [ "$EXT" = "1" ] && [ "$V1" = "$V5" ]; then C5=1; fi
  else
    log "pg_restore reportou erros (ver acima)"
  fi
else
  log "spike-c5 não ficou ready"
fi

# ---------------------------------------------------------------- MATRIZ
TOTAL=0
for c in "$C1" "$C2" "$C3" "$C4" "$C5"; do TOTAL=$((TOTAL + c)); done
log ""
log "======================================================================"
log "MATRIZ SPIKE 3f.0 — $TOTAL/5"
log "  1. pgvector/pgvector:pg16 sobe com volume postgres:16 sem dump/restore, CREATE EXTENSION ok .... $( [ "$C1" = 1 ] && echo PASS || echo FAIL )"
log "  2. crate pgvector (sqlx runtime): round-trip 512d + ORDER BY v <=> \$1 ordena certo .............. $( [ "$C2" = 1 ] && echo PASS || echo FAIL )"
log "  3. HNSW 512d 10k: build<30s, k=10 <50ms (p50), top-1 ≥90% vs brute force ....................... $( [ "$C3" = 1 ] && echo PASS || echo FAIL )"
log "  4. embedder mock /embed(batch32)+/embed-text < 500ms cada, vetor == mock Rust .................. $( [ "$C4" = 1 ] && echo PASS || echo FAIL )"
log "  5. pg_dump com vetores -> pg_restore preserva o tipo e os valores .............................. $( [ "$C5" = 1 ] && echo PASS || echo FAIL )"
log "======================================================================"
[ "$TOTAL" = 5 ]
