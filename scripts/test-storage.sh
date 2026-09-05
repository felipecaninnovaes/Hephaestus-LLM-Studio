#!/usr/bin/env bash
# Integração storage S3 × SeaweedFS (Fatia 3b.4, testes --ignored).
# Uso: bash scripts/test-storage.sh
#   sobe só o `seaweedfs` do compose (se já não estava rodando), espera o
#   S3 no ar (~60s), roda `cargo test -p api-principal --test storage_s3
#   -- --ignored` e derruba SÓ o que subiu. O `db` NÃO é necessário (os
#   testes exercem a porta S3Storage, sem pool). Nunca toca volumes de
#   outros projetos (infra_postgres_data, infra_minio_data, ... não são
#   deste repo).
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
compose() { docker compose -f "$ROOT_DIR/infra/compose.yaml" "$@"; }

# 1. Estado anterior: espelha o test-db.sh — RUNNING != EXISTENTE: um
#    container STOPPED do usuário não pode ser rm -f'd (revisão 3b.6).
if [[ -n "$(compose ps -q seaweedfs 2>/dev/null || true)" ]]; then
  WAS_RUNNING=1
else
  WAS_RUNNING=0
fi
if [[ -n "$(compose ps -aq seaweedfs 2>/dev/null || true)" ]]; then
  HAD_CONTAINER=1
else
  HAD_CONTAINER=0
fi
if docker volume ls -q 2>/dev/null | grep -qx 'infra_seaweed_data'; then
  HAD_VOLUME=1
else
  HAD_VOLUME=0
fi
echo "antes: seaweedfs_running=$WAS_RUNNING seaweedfs_container=$HAD_CONTAINER volume_infra_seaweed_data=$HAD_VOLUME"

TEST_CODE=0
OUT_FILE="$(mktemp)"

cleanup() {
  # 5. Sempre: desfaz SÓ o que o script criou.
  if [[ "$WAS_RUNNING" == "0" && "$HAD_CONTAINER" == "0" ]]; then
    compose stop seaweedfs >/dev/null 2>&1 || true
    compose rm -f seaweedfs >/dev/null 2>&1 || true
    echo "depois: seaweedfs parado e container removido (script subiu)"
  elif [[ "$WAS_RUNNING" == "0" ]]; then
    compose stop seaweedfs >/dev/null 2>&1 || true
    echo "depois: seaweedfs parado (container pré-existente mantido, sem rm)"
  else
    echo "depois: seaweedfs era pré-existente e segue rodando (não tocado)"
  fi
  if [[ "$HAD_VOLUME" == "0" ]]; then
    if docker volume ls -q 2>/dev/null | grep -qx 'infra_seaweed_data'; then
      docker volume rm infra_seaweed_data >/dev/null 2>&1 || true
      echo "depois: volume infra_seaweed_data removido (script criou)"
    else
      echo "depois: volume infra_seaweed_data já sumiu sozinho"
    fi
  else
    echo "depois: volume infra_seaweed_data pré-existente (não tocado)"
  fi
  rm -f "$OUT_FILE"
}
trap cleanup EXIT

# 2. Sobe só o SeaweedFS.
compose up -d seaweedfs

# 3. Poll de prontidão ~60s. S3 anônimo em `/` responde 403 (SigV4 exigido)
#    — qualquer código HTTP != 000 significa API no ar; 000 = recusada.
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

# 4. Roda os testes --ignored contra o seaweedfs do host, guardando o exit
#    code. --show-output: os gates STORAGE_BIG/STORAGE_DEAD imprimem SKIP que
#    o cargo contaria como "ok" mudo — aqui os SKIPs ficam visíveis e o
#    resumo abaixo explicita a super-contagem (revisão 3b.6 #9/F7).
cd "$ROOT_DIR"
TEST_CODE=0
S3_ENDPOINT_URL="http://localhost:8333" \
S3_PUBLIC_ENDPOINT_URL="http://localhost:8333" \
S3_BUCKET="${S3_BUCKET:-heph-data}" \
S3_ACCESS_KEY="${S3_ACCESS_KEY:-heph}" \
S3_SECRET_KEY="${S3_SECRET_KEY:-heph-local-dev}" \
  cargo test -p api-principal --test storage_s3 -- --ignored --show-output 2>&1 | tee "$OUT_FILE" || TEST_CODE=$?

SKIPS="$(grep -c 'SKIP' "$OUT_FILE" || true)"
if [[ "$SKIPS" != "0" ]]; then
  echo "nota: $SKIPS linha(s) de SKIP — gates STORAGE_BIG=1/STORAGE_DEAD=1 não ligados (super-contagem no 'passed')"
fi

# 6. Exit com o código do cargo test (o trap limpa antes).
exit "$TEST_CODE"
