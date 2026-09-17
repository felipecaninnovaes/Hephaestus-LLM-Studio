#!/usr/bin/env bash
# Smoke e2e: /health dos 3 serviços (default) + fluxo auth (--auth) +
# fluxo datasets/storage (--datasets).
# Uso: bash scripts/e2e-smoke.sh [--auth|--datasets]
#   --auth assume api-principal em :8080 com STUDIO_PASSWORD=teste-01
#   (compose.integ.yaml) sobre Postgres: poll /health até auth:"ready"
#   (~60s) → login 200 + set-cookie → me 200 → logout 204 → me 401.
#   --datasets exige a stack rodando (db + principal + seaweedfs do compose
#   com STORAGE_BACKEND=s3, STUDIO_PASSWORD=teste-01): prova o WIRED
#   commit→sweep→204 (a prova byte-a-byte do sweep — prefixo vazio no bucket
#   com listagem paginada e assinatura real SigV4 — vive em
#   `scripts/test-storage.sh`; curl anônimo não passa do 403 SigV4, por isso
#   aqui o assert final é lista sem o id + GET images 404, não HEAD no bucket).
# Nota D5: logout só limpa o cookie no client (sem blacklist); o 401 final
# vem do jar já sem sessão (curl -c atualiza o jar no logout), não do servidor
# invalidar o JWT — que segue assinado até exp.
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
compose() { docker compose -f "$ROOT_DIR/infra/compose.yaml" "$@"; }

if [[ "${1:-}" == "--datasets" ]]; then
  API="http://localhost:8080"
  PASS="${STUDIO_PASSWORD:-teste-01}"
  echo "--datasets: fluxo create → upload → duplicate → images → data → boxes → ready → delete"
  ready=0
  for _ in $(seq 1 60); do
    if curl -fsS "$API/health" 2>/dev/null | grep -q '"auth"[[:space:]]*:[[:space:]]*"ready"'; then
      ready=1
      break
    fi
    sleep 1
  done
  if [[ "$ready" != "1" ]]; then
    echo "timeout esperando /health auth:\"ready\"" >&2
    exit 1
  fi
  echo "auth: ready"
  TMPDIR_SMOKE="$(mktemp -d)"
  trap 'rm -rf "$TMPDIR_SMOKE"' EXIT
  JAR="$TMPDIR_SMOKE/jar"
  BODY="$TMPDIR_SMOKE/body"
  PNG_B64="iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg=="
  jget() { python3 -c "import json,sys; print(json.load(open('$BODY'))$1)"; }
  # 1. login (jar).
  code="$(curl -sS -o "$BODY" -w "%{http_code}" -c "$JAR" \
    -X POST "$API/api/auth/login" \
    -H 'Content-Type: application/json' \
    -d "{\"password\":\"$PASS\"}")"
  [[ "$code" == "200" ]]
  echo "login: 200"
  # 2. POST /api/datasets ⇒ 201, captura id + classes[0].id. Título com
  # epoch: slug único por execução (título fixo envenenava re-runs com 409).
  TITLE="E2E Storage $(date +%s)"
  code="$(curl -sS -o "$BODY" -w "%{http_code}" -b "$JAR" \
    -X POST "$API/api/datasets" \
    -H 'Content-Type: application/json' \
    -d "{\"title\":\"$TITLE\",\"type\":\"yolo_bbox\",\"classes\":[\"obj\"]}")"

  [[ "$code" == "201" ]]
  DS_ID="$(jget "['id']")"
  CLASS_ID="$(jget "['classes'][0]['id']")"
  [[ -n "$DS_ID" && -n "$CLASS_ID" ]]
  echo "create: 201 title=$TITLE id=$DS_ID classId=$CLASS_ID"
  # 3. PNG 1×1 do fixture (mesmo dos testes).
  python3 -c "import base64; open('$TMPDIR_SMOKE/a.png','wb').write(base64.b64decode('$PNG_B64'))"
  # 4. upload ⇒ stored; reenvio do MESMO arquivo ⇒ duplicate. Retry c/
  # settle S3: PUTs reais dão 500/transiente nos primeiros segundos pós-boot
  # mesmo com bucket garantido + /health ready — 5 tentativas antes do assert.
  code="000"
  for i in $(seq 1 5); do
    code="$(curl -sS -o "$BODY" -w "%{http_code}" -b "$JAR" \
      -X POST "$API/api/datasets/$DS_ID/upload" \
      -F "files=@$TMPDIR_SMOKE/a.png" || true)"
    [[ -z "$code" ]] && code="000"
    [[ "$code" == "200" ]] && break
    [[ "$i" -lt 5 ]] && { echo "aguardando settle S3 ($i/5, HTTP $code)" >&2; sleep 2; }
  done
  [[ "$code" == "200" ]]
  [[ "$(jget "['items'][0]['status']")" == "stored" ]]
  IMG_ID="$(jget "['items'][0]['imageId']")"
  echo "upload: stored imageId=$IMG_ID"
  code="$(curl -sS -o "$BODY" -w "%{http_code}" -b "$JAR" \
    -X POST "$API/api/datasets/$DS_ID/upload" \
    -F "files=@$TMPDIR_SMOKE/a.png")"
  [[ "$code" == "200" ]]
  [[ "$(jget "['items'][0]['status']")" == "duplicate" ]]
  echo "reenvio: duplicate (UNIQUE+compensação)"
  # 5. images ⇒ total 1, url presigned; dataset ⇒ source s3://, imagesCount 1.
  code="$(curl -sS -o "$BODY" -w "%{http_code}" -b "$JAR" \
    "$API/api/datasets/$DS_ID/images")"
  [[ "$code" == "200" ]]
  [[ "$(jget "['total']")" == "1" ]]
  IMG_URL="$(jget "['items'][0]['url']")"
  case "$IMG_URL" in *X-Amz-Signature*) ;; *) echo "url sem X-Amz-Signature: $IMG_URL" >&2; exit 1;; esac
  echo "images: total 1 + presigned"
  code="$(curl -sS -o "$BODY" -w "%{http_code}" -b "$JAR" \
    "$API/api/datasets/$DS_ID")"
  [[ "$code" == "200" ]]
  case "$(jget "['source']")" in s3://*) ;; *) echo "source sem s3://: $(jget "['source']")" >&2; exit 1;; esac
  [[ "$(jget "['imagesCount']")" == "1" ]]
  [[ "$(jget "['status']")" == "needs_labeling" ]]
  echo "dataset: source s3:// + imagesCount 1 + needs_labeling"
  # 6. /data ⇒ contrato real pós-ADR-0017: upload normaliza p/ WebP, então
  # os bytes servidos NUNCA são idênticos ao PNG de entrada (sha contra o
  # fixture morreu aqui). Asserta 200 + magic RIFF....WEBP + sha256 estável
  # entre dois GETs consecutivos (determinismo do objeto servido).
  code="$(curl -sS -o "$TMPDIR_SMOKE/got1.bin" -w "%{http_code}" -b "$JAR" \
    "$API/api/datasets/$DS_ID/images/$IMG_ID/data" || true)"
  [[ "$code" == "200" ]]
  magic="$(od -An -tx1 -N12 "$TMPDIR_SMOKE/got1.bin" | tr -d ' \n')"
  case "$magic" in 52494646????????57454250) ;; *) echo "magic sem RIFF....WEBP: $magic" >&2; exit 1;; esac
  code="$(curl -sS -o "$TMPDIR_SMOKE/got2.bin" -w "%{http_code}" -b "$JAR" \
    "$API/api/datasets/$DS_ID/images/$IMG_ID/data" || true)"
  [[ "$code" == "200" ]]
  sha1="$(sha256sum "$TMPDIR_SMOKE/got1.bin" | cut -d' ' -f1)"
  sha2="$(sha256sum "$TMPDIR_SMOKE/got2.bin" | cut -d' ' -f1)"
  [[ -n "$sha1" && "$sha1" == "$sha2" ]]
  echo "data: webp servido + sha estavel $sha1"
  # 7. PUT boxes com classId ⇒ 200; dataset ⇒ ready.
  code="$(curl -sS -o "$BODY" -w "%{http_code}" -b "$JAR" \
    -X PUT "$API/api/datasets/$DS_ID/images/$IMG_ID/boxes" \
    -H 'Content-Type: application/json' \
    -d "{\"boxes\":[{\"classId\":\"$CLASS_ID\",\"x\":0.5,\"y\":0.5,\"w\":0.2,\"h\":0.2}]}")"
  [[ "$code" == "200" ]]
  code="$(curl -sS -o "$BODY" -w "%{http_code}" -b "$JAR" \
    "$API/api/datasets/$DS_ID")"
  [[ "$code" == "200" ]]
  [[ "$(jget "['status']")" == "ready" ]]
  echo "boxes: 200 + status ready"
  # 8. DELETE ⇒ 204; images ⇒ 404; lista sem o id.
  code="$(curl -sS -o "$BODY" -w "%{http_code}" -b "$JAR" \
    -X DELETE "$API/api/datasets/$DS_ID")"
  [[ "$code" == "204" ]]
  code="$(curl -sS -o "$BODY" -w "%{http_code}" -b "$JAR" \
    "$API/api/datasets/$DS_ID/images")"
  [[ "$code" == "404" ]]
  code="$(curl -sS -o "$BODY" -w "%{http_code}" -b "$JAR" \
    "$API/api/datasets")"
  [[ "$code" == "200" ]]
  if grep -q "$DS_ID" "$BODY"; then echo "id ainda listado após DELETE" >&2; exit 1; fi
  echo "delete: 204 + images 404 + fora da lista"
  exit 0
fi

if [[ "${1:-}" != "--auth" ]]; then
  for svc in 8080 8081 8082; do
    curl -fsS "http://localhost:${svc}/health" && echo " :${svc} ok"
  done
  exit 0
fi

API="http://localhost:8080"
PASS="${STUDIO_PASSWORD:-teste-01}"
echo "--auth: fluxo login → me → logout → me 401"

# Poll /health até auth:"ready" (timeout ~60s).
ready=0
for _ in $(seq 1 60); do
  if curl -fsS "$API/health" 2>/dev/null | grep -q '"auth"[[:space:]]*:[[:space:]]*"ready"'; then
    ready=1
    break
  fi
  sleep 1
done
if [[ "$ready" != "1" ]]; then
  echo "timeout esperando /health auth:\"ready\"" >&2
  exit 1
fi
echo "auth: ready"

TMPDIR_SMOKE="$(mktemp -d)"
trap 'rm -rf "$TMPDIR_SMOKE"' EXIT
JAR="$TMPDIR_SMOKE/jar"
HEAD="$TMPDIR_SMOKE/head"
BODY="$TMPDIR_SMOKE/body"

# POST login senha errada → 401 invalid_credentials.
code="$(curl -sS -o "$BODY" -w "%{http_code}" \
  -X POST "$API/api/auth/login" \
  -H 'Content-Type: application/json' \
  -d '{"password":"errada-de-proposito"}')"
[[ "$code" == "401" ]]
grep -q '"invalid_credentials"' "$BODY"
echo "login senha errada: 401"

# POST login → 200 + set-cookie: heph_session=.
code="$(curl -sS -D "$HEAD" -o "$BODY" -w "%{http_code}" -c "$JAR" \
  -X POST "$API/api/auth/login" \
  -H 'Content-Type: application/json' \
  -d "{\"password\":\"$PASS\"}")"
grep -qi 'set-cookie:[[:space:]]*heph_session=' "$HEAD"
[[ "$code" == "200" ]]
echo "login: 200 + heph_session"

# GET /api/auth/me com cookie → 200.
code="$(curl -sS -o "$BODY" -w "%{http_code}" -b "$JAR" "$API/api/auth/me")"
[[ "$code" == "200" ]]
echo "me: 200"

# POST logout → 204 (jar atualizado: sessão limpa no client).
code="$(curl -sS -D "$HEAD" -o "$BODY" -w "%{http_code}" -b "$JAR" -c "$JAR" \
  -X POST "$API/api/auth/logout")"
[[ "$code" == "204" ]]
grep -qi 'Max-Age=0' "$HEAD"
echo "logout: 204"

# GET me com o MESMO jar (agora sem sessão) → 401.
code="$(curl -sS -o "$BODY" -w "%{http_code}" -b "$JAR" "$API/api/auth/me")"
[[ "$code" == "401" ]]
echo "me pós-logout: 401"
