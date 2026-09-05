#!/usr/bin/env bash
# Integração storage S3 × SeaweedFS (Fatia 3b.4, testes --ignored).
# Uso: bash scripts/test-storage.sh
#   sobe `seaweedfs` + `db` do compose (se já não estavam rodando), espera
#   healthy (~60s), roda `cargo test -p api-principal --test storage_s3 -- --ignored`
#   e derruba SÓ o que subiu. Nunca toca volumes de outros projetos
#   (infra_postgres_data, infra_minio_data, ... não são deste repo).
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
compose() { docker compose -f "$ROOT_DIR/infra/compose.yaml" "$@"; }

# 1. Estado anterior: seaweedfs/db deste compose rodando? volume infra_seaweed_data existia?
if [[ -n "$(compose ps -q seaweedfs 2>/dev/null || true)" ]]; then
  SW_WAS_RUNNING=1
else
  SW_WAS_RUNNING=0
fi
if [[ -n "$(compose ps -q db 2>/dev/null || true)" ]]; then
  DB_WAS_RUNNING=1
else
  DB_WAS_RUNNING=0
fi
if docker volume ls -q 2>/dev/null | grep -qx 'infra_seaweed_data'; then
  HAD_VOLUME=1
else
  HAD_VOLUME=0
fi
echo "antes: seaweedfs_running=$SW_WAS_RUNNING db_running=$DB_WAS_RUNNING volume_infra_seaweed_data=$HAD_VOLUME"

TEST_CODE=0

cleanup() {
  # 5. Sempre: desfaz SÓ o que o script criou.
  if [[ "$SW_WAS_RUNNING" == "0" ]]; then
    compose stop seaweedfs >/dev/null 2>&1 || true
    compose rm -f seaweedfs >/dev/null 2>&1 || true
    echo "depois: seaweedfs parado e container removido (script subiu)"
  else
    echo "depois: seaweedfs era pré-existente e segue rodando (não tocado)"
  fi
  if [[ "$DB_WAS_RUNNING" == "0" ]]; then
    compose stop db >/dev/null 2>&1 || true
    compose rm -f db >/dev/null 2>&1 || true
    echo "depois: db parado e container removido (script subiu)"
  else
    echo "depois: db era pré-existente e segue rodando (não tocado)"
  fi
  if [[ "$HAD_VOLUME" == "0" ]]; then
    if docker volume ls -q 2>/dev/null | grep -qx 'infra_seaweed_data'; then
      docker volume rm infra_seaweed_data >/dev/null
      echo "depois: volume infra_seaweed_data removido (script criou)"
    else
      echo "depois: volume infra_seaweed_data já sumiu sozinho"
    fi
  else
    echo "depois: volume infra_seaweed_data pré-existente (não tocado)"
  fi
}
trap cleanup EXIT

# 2. Sobe o SeaweedFS + db.
compose up -d seaweedfs db

# 3. Poll de prontidão ~60s (healthcheck do seaweedfs + pg_isready do db).
# S3 anônimo em `/` responde 403 (SigV4 exigido) — qualquer código HTTP != 000
# significa API no ar; 000 = conexão recusada.
ready=0
for _ in $(seq 1 60); do
  code="$(curl -s -o /dev/null -w '%{http_code}' http://localhost:8333/ 2>/dev/null || true)"
  if [[ "$code" != "000" && -n "$code" ]]; then
    ready=1
    break
  fi
  sleep 1
done
if [[ "$ready" != "1" ]]; then
  echo "timeout esperando S3 do seaweedfs em localhost:8333 (~60s)" >&2
  exit 1
fi
echo "seaweedfs: S3 ok em localhost:8333"

dbready=0
for _ in $(seq 1 60); do
  if compose exec -T db pg_isready -U studio -d studio >/dev/null 2>&1; then
    dbready=1
    break
  fi
  sleep 1
done
if [[ "$dbready" != "1" ]]; then
  echo "timeout esperando pg_isready do db (~60s)" >&2
  exit 1
fi
echo "db: pg_isready ok"

# 4. Roda os testes --ignored contra o seaweedfs do host, guardando o exit code.
cd "$ROOT_DIR"
TEST_CODE=0
S3_ENDPOINT_URL="http://localhost:8333" \
S3_PUBLIC_ENDPOINT_URL="http://localhost:8333" \
S3_BUCKET="${S3_BUCKET:-heph-data}" \
S3_ACCESS_KEY="${S3_ACCESS_KEY:-heph}" \
S3_SECRET_KEY="${S3_SECRET_KEY:-heph-local-dev}" \
  cargo test -p api-principal --test storage_s3 -- --ignored || TEST_CODE=$?

# 6. Exit com o código do cargo test (o trap limpa antes).
exit "$TEST_CODE"
