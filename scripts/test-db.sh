#!/usr/bin/env bash
# Integração datasets × Postgres (Fatia 3a, testes --ignored).
# Uso: bash scripts/test-db.sh
#   sobe só o `db` do compose (se já não estava rodando), espera pg_isready
#   (~60s), roda `cargo test -p api-principal --test datasets_db -- --ignored`
#   e derruba SÓ o que subiu. Nunca toca volumes de outros projetos
#   (infra_postgres_data, infra_minio_data, ... não são deste repo).
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
compose() { docker compose -f "$ROOT_DIR/infra/compose.yaml" "$@"; }

# 1. Estado anterior: db deste compose rodando? volume infra_pgdata existia?
if [[ -n "$(compose ps -q db 2>/dev/null || true)" ]]; then
  WAS_RUNNING=1
else
  WAS_RUNNING=0
fi
if [[ -n "$(compose ps -aq db 2>/dev/null || true)" ]]; then
  HAD_CONTAINER=1
else
  HAD_CONTAINER=0
fi
if docker volume ls -q 2>/dev/null | grep -qx 'infra_pgdata'; then
  HAD_VOLUME=1
else
  HAD_VOLUME=0
fi
echo "antes: db_running=$WAS_RUNNING db_container=$HAD_CONTAINER volume_infra_pgdata=$HAD_VOLUME"

TEST_CODE=0

cleanup() {
  # 5. Sempre: desfaz SÓ o que o script criou.
  if [[ "$WAS_RUNNING" == "0" && "$HAD_CONTAINER" == "0" ]]; then
    compose stop db >/dev/null 2>&1 || true
    compose rm -f db >/dev/null 2>&1 || true
    echo "depois: db parado e container removido (script subiu)"
  elif [[ "$WAS_RUNNING" == "0" ]]; then
    compose stop db >/dev/null 2>&1 || true
    echo "depois: db parado (container pré-existente mantido, sem rm)"
  else
    echo "depois: db era pré-existente e segue rodando (não tocado)"
  fi
  if [[ "$HAD_VOLUME" == "0" ]]; then
    if docker volume ls -q 2>/dev/null | grep -qx 'infra_pgdata'; then
      docker volume rm infra_pgdata >/dev/null
      echo "depois: volume infra_pgdata removido (script criou)"
    else
      echo "depois: volume infra_pgdata já sumiu sozinho"
    fi
  else
    echo "depois: volume infra_pgdata pré-existente (não tocado)"
  fi
}
trap cleanup EXIT

# 2. Sobe só o banco.
compose up -d db

# 3. Poll de prontidão ~60s (o compose não tem healthcheck no db).
ready=0
for _ in $(seq 1 60); do
  if compose exec -T db pg_isready -U studio -d studio >/dev/null 2>&1; then
    ready=1
    break
  fi
  sleep 1
done
if [[ "$ready" != "1" ]]; then
  echo "timeout esperando pg_isready do db (~60s)" >&2
  exit 1
fi
echo "db: pg_isready ok"

# 4. Roda os testes --ignored, guardando o exit code.
cd "$ROOT_DIR"
TEST_CODE=0
DATABASE_URL="postgres://studio:${POSTGRES_PASSWORD:-studio}@localhost:5432/studio" \
  cargo test -p api-principal --test datasets_db -- --ignored || TEST_CODE=$?

# 5. Testes do manager (F4.3) — mesmo banco, MESMO exit code guardado.
DATABASE_URL="postgres://studio:${POSTGRES_PASSWORD:-studio}@localhost:5432/studio" \
  cargo test -p manager --test manager_db -- --ignored || TEST_CODE=$?

# 6. Exit com o código do cargo test (o trap limpa antes).
exit "$TEST_CODE"
