#!/usr/bin/env bash
# Smoke e2e: /health dos 3 serviços (default) + fluxo auth (--auth) no integ.
# Uso: bash scripts/e2e-smoke.sh [--auth]
#   --auth assume api-principal em :8080 com STUDIO_PASSWORD=teste-01
#   (compose.integ.yaml) sobre Postgres: poll /health até auth:"ready"
#   (~60s) → login 200 + set-cookie → me 200 → logout 204 → me 401.
# Nota D5: logout só limpa o cookie no client (sem blacklist); o 401 final
# vem do jar já sem sessão (curl -c atualiza o jar no logout), não do servidor
# invalidar o JWT — que segue assinado até exp.
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
compose() { docker compose -f "$ROOT_DIR/infra/compose.yaml" "$@"; }

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
