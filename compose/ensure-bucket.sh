#!/usr/bin/env sh
# ==============================================================================
# Hephaestus LLM Studio — garante o bucket S3 (heph-data) no SeaweedFS.
#
# PORQUÊ: `docker compose down -v` apaga o volume `seaweed_data` e o bucket
# some junto. A identidade S3 volta via infra/seaweedfs-s3.json (bind-mount),
# mas o BUCKET não é auto-criado para quem escreve com identidade escopada
# (heph-orch só tem Read/Write/List em prefixos de heph-data — o auto-create
# do SeaweedFS (-s3.autoCreateBucket) só acontece no 1º PUT de identidade
# Admin). Sem bucket, os uploads do orquestrador falham silenciosamente e os
# jobs (ex.: diffusion_generate) ficam "done" com ZERO artefatos.
#
# Uso chamado pelo serviço compose `s3-init` (dependência de principal,
# manager e orchestrator-local) e como preflight WARN-only de
# scripts/run-native.sh. Idempotente: 200 OK ou BucketAlreadyOwnedByYou /
# BucketAlreadyExists = sucesso; qualquer outro resultado = exit 1.
#
# Sem dependências no host além de curl+openssl (nada de aws-cli/boto3): o
# PUT /{bucket}/ é assinado à mão (SigV4, serviço s3, região us-east-1).
# POSIX sh de propósito: roda no `sh` do busybox do alpine do s3-init sem
# instalar bash (só `apk add curl openssl`).
#
# Env (todas sobrescrevíveis; defaults = LOCAL-DEV do compose, documentadas
# em infra/seaweedfs-s3.json — rotacione antes de expor além do loopback):
#   S3_ENDPOINT_URL  default http://localhost:8333
#   S3_BUCKET        default heph-data
#   S3_ACCESS_KEY    default heph
#   S3_SECRET_KEY    default heph-local-dev
# ==============================================================================

set -eu

usage() {
  cat <<'EOF'
Uso: ensure-bucket.sh [--help]

Garante (cria se ausente) o bucket S3 no endpoint SeaweedFS via PUT
assinado SigV4. Idempotente — seguro rodar a cada boot.

Env: S3_ENDPOINT_URL (default http://localhost:8333),
     S3_BUCKET (default heph-data),
     S3_ACCESS_KEY (default heph), S3_SECRET_KEY (default heph-local-dev).

Saída: linha PT de ok/erro. Exit 0 se o bucket existe (criado agora ou já
existia); exit 1 em qualquer outro caso. Requer curl + openssl no PATH.
EOF
}

if [ "${1:-}" = "--help" ] || [ "${1:-}" = "-h" ]; then
  usage
  exit 0
fi
if [ $# -gt 0 ]; then
  echo "ensure-bucket.sh: opção desconhecida: $1 (use --help)" >&2
  exit 1
fi

command -v curl >/dev/null 2>&1 || { echo "ensure-bucket.sh: erro: curl ausente no PATH." >&2; exit 1; }
command -v openssl >/dev/null 2>&1 || { echo "ensure-bucket.sh: erro: openssl ausente no PATH." >&2; exit 1; }

S3_ENDPOINT_URL="${S3_ENDPOINT_URL:-http://localhost:8333}"
S3_BUCKET="${S3_BUCKET:-heph-data}"
S3_ACCESS_KEY="${S3_ACCESS_KEY:-heph}"
S3_SECRET_KEY="${S3_SECRET_KEY:-heph-local-dev}"
REGION="us-east-1"
SERVICE="s3"
EMPTY_SHA="e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"

ENDPOINT="$(printf '%s' "$S3_ENDPOINT_URL" | sed 's|/$||')"
HOSTHDR="$(printf '%s' "$ENDPOINT" | sed -e 's|^[A-Za-z][A-Za-z0-9+.-]*://||')"
NOW="$(date -u +%Y%m%dT%H%M%SZ)"
AMZDATE="$NOW"
DATESTAMP="${NOW%T*}"
CANONICAL_URI="/${S3_BUCKET}/"
SIGNED_HEADERS="host;x-amz-content-sha256;x-amz-date"

CANONICAL_REQ="PUT
${CANONICAL_URI}

host:${HOSTHDR}
x-amz-content-sha256:${EMPTY_SHA}
x-amz-date:${AMZDATE}

${SIGNED_HEADERS}
${EMPTY_SHA}"
CANONICAL_HASH="$(printf '%s' "$CANONICAL_REQ" | openssl dgst -sha256 | awk '{print $NF}')"
STRING_TO_SIGN="AWS4-HMAC-SHA256
${AMZDATE}
${DATESTAMP}/${REGION}/${SERVICE}/aws4_request
${CANONICAL_HASH}"

KDATE="$(printf '%s' "$DATESTAMP" | openssl dgst -sha256 -hmac "AWS4${S3_SECRET_KEY}" | awk '{print $NF}')"
KREGION="$(printf '%s' "$REGION" | openssl dgst -sha256 -mac HMAC -macopt "hexkey:${KDATE}" | awk '{print $NF}')"
KSERVICE="$(printf '%s' "$SERVICE" | openssl dgst -sha256 -mac HMAC -macopt "hexkey:${KREGION}" | awk '{print $NF}')"
KSIGNING="$(printf '%s' 'aws4_request' | openssl dgst -sha256 -mac HMAC -macopt "hexkey:${KSERVICE}" | awk '{print $NF}')"
SIGNATURE="$(printf '%s' "$STRING_TO_SIGN" | openssl dgst -sha256 -mac HMAC -macopt "hexkey:${KSIGNING}" | awk '{print $NF}')"
AUTH="AWS4-HMAC-SHA256 Credential=${S3_ACCESS_KEY}/${DATESTAMP}/${REGION}/${SERVICE}/aws4_request, SignedHeaders=${SIGNED_HEADERS}, Signature=${SIGNATURE}"

BODY_FILE="$(mktemp)"
trap 'rm -f "$BODY_FILE"' EXIT INT TERM
HTTP_CODE="$(curl -s -o "$BODY_FILE" -w '%{http_code}' --max-time 10 -X PUT \
  "${ENDPOINT}${CANONICAL_URI}" \
  -H "Host: ${HOSTHDR}" \
  -H "x-amz-date: ${AMZDATE}" \
  -H "x-amz-content-sha256: ${EMPTY_SHA}" \
  -H "Authorization: ${AUTH}" || true)"
# curl falha (ex.: conexão recusada) => sem HTTP_CODE; o `|| true` acima
# impede o `set -e` de abortar antes da nossa mensagem PT (bash aborta em
# substituição de comando falha, busybox-ash não — comportamento unificado).
[ -n "$HTTP_CODE" ] || HTTP_CODE="000"
BODY_HEAD="$(head -c 300 "$BODY_FILE" | tr -d '\0')"

if [ "$HTTP_CODE" = "200" ]; then
  echo "ensure-bucket.sh: ok — bucket '${S3_BUCKET}' garantido em ${ENDPOINT} (HTTP 200)."
  exit 0
fi
case "$BODY_HEAD" in
  *"<Code>BucketAlreadyOwnedByYou</Code>"*|*"<Code>BucketAlreadyExists</Code>"*)
    echo "ensure-bucket.sh: ok — bucket '${S3_BUCKET}' já existia em ${ENDPOINT} (idempotente)."
    exit 0
    ;;
esac
echo "ensure-bucket.sh: erro — PUT ${ENDPOINT}${CANONICAL_URI} falhou (HTTP ${HTTP_CODE}): ${BODY_HEAD:-sem resposta do endpoint — fora do ar?}" >&2
exit 1
