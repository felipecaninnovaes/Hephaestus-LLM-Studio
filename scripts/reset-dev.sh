#!/usr/bin/env bash
# ============================================================================
# Hephaestus LLM Studio — PROVA day-one: reset destrutivo + asserts bootstrap.
# GRAVE — DESTRUTIVO: apaga TODOS os volumes dev (pgdata, seaweed_data,
# datasets, models, outputs). Banco, bucket S3 e artefatos SOMEM. Valida que o
# sistema se levanta SOZINHO do zero. NUNCA rode contra dados que importam.
# down -v → up -d → espera s3-init + /health auth=ready → ASSERTS → PASS/FAIL.
# Exit 0 SÓ com todos PASS. Sem sudo; só volumes infra_* (down -v já cuida).
# Limitação: orquestradores REMOTOS (TrueNAS) exigem re-adoção manual pós-reset
# (pairing não automatizado). Sem --yes: lista o que faria e sai 1 (trava).
# Uso: bash scripts/reset-dev.sh --yes [--skip-smoke] [--build] [--with-web] [--timeout 180]
# ============================================================================
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
COMPOSE_FILE="$ROOT_DIR/infra/compose.yaml"
ENV_FILE="$ROOT_DIR/infra/.env"
API="http://localhost:8080"

DO_YES=false; SKIP_SMOKE=false; DO_BUILD=false; WITH_WEB=false; TIMEOUT=180

# Backend explícito (paridade com start-host.sh --no-web): web FORA por
# default — o dev roda `npm run dev` no host :3000 e o container web quebraria
# o up com FAIL espúrio. --with-web inclui o web (modo full).
BACKEND_SERVICES=(db seaweedfs embedder principal manager orchestrator-local)

usage() {
  echo "Uso: scripts/reset-dev.sh --yes [--skip-smoke] [--build] [--with-web] [--timeout <secs>]"
  echo "PROVA day-one (destrutiva): down -v + up -d + asserts de bootstrap do zero."
  echo "  --yes  confirma (sem ela: lista e sai 1); --skip-smoke pula smoke final"
  echo "  --build  up com --build; --with-web inclui container web (default: só backend)"
  echo "  --timeout <secs> espera total (default 180)"
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --yes) DO_YES=true; shift ;;
    --skip-smoke) SKIP_SMOKE=true; shift ;;
    --build) DO_BUILD=true; shift ;;
    --with-web) WITH_WEB=true; shift ;;
    --timeout)
      [[ $# -ge 2 ]] || { echo "Erro: --timeout exige valor." >&2; exit 1; }
      TIMEOUT="$2"; shift 2 ;;
    --help|-h) usage; exit 0 ;;
    *) echo "Opção desconhecida: $1 (use --help)" >&2; exit 1 ;;
  esac
done
[[ "$TIMEOUT" =~ ^[0-9]+$ ]] || { echo "Erro: --timeout deve ser inteiro." >&2; exit 1; }

# compose com --env-file só se infra/.env existir (mesma regra dos vizinhos).
compose() {
  if [[ -f "$ENV_FILE" ]]; then docker compose -f "$COMPOSE_FILE" --env-file "$ENV_FILE" "$@"
  else docker compose -f "$COMPOSE_FILE" "$@"; fi
}

if [[ "$DO_YES" != true ]]; then
  echo "DRY-RUN (nada executado — sem --yes, exit 1 por trava de segurança):"
  echo "  [1] Pré: docker daemon + STUDIO_PASSWORD (gera se ausente)"
  echo "  [2] down -v --remove-orphans (apaga volumes dev infra_*)"
  echo "  [3] up -d ${BACKEND_SERVICES[*]}$([[ "$WITH_WEB" == true ]] && echo ' web' || true)$([[ "$DO_BUILD" == true ]] && echo ' --build' || echo ' (imagens atuais)')"
  echo "  [4] s3-init completed + poll $API/health até auth=ready (${TIMEOUT}s)"
  echo "  [5] ASSERTS: health, ensure-bucket.sh, login, restart-loop, smoke e2e"
  exit 1
fi

PASS_COUNT=0; FAIL_COUNT=0; RESULTS=()
report() { # $1=PASS|FAIL $2=[n/6] $3=msg
  echo "$1: $2 $3"; RESULTS+=("$1: $2 $3")
  if [[ "$1" == "PASS" ]]; then PASS_COUNT=$((PASS_COUNT+1)); else FAIL_COUNT=$((FAIL_COUNT+1)); fi
}

# --- [1/6] Pré: daemon + senha ---
if docker info >/dev/null 2>&1; then
  report PASS "[1/6]" "docker daemon acessível"
else
  report FAIL "[1/6]" "docker daemon inacessível (docker info falhou)"
fi
GENERATED=false
if [[ -z "${STUDIO_PASSWORD:-}" ]]; then
  if command -v openssl >/dev/null 2>&1; then STUDIO_PASSWORD="$(openssl rand -hex 16)"
  else STUDIO_PASSWORD="$(head -c 16 /dev/urandom | od -An -tx1 | tr -d ' \n')"; fi
  GENERATED=true
fi
export STUDIO_PASSWORD
report PASS "[1/6]" "STUDIO_PASSWORD pronta (gerada=${GENERATED})"

# --- [2/6] down -v ---
if DOWN_OUT="$(STUDIO_PASSWORD="$STUDIO_PASSWORD" compose down -v --remove-orphans 2>&1)"; then
  report PASS "[2/6]" "down -v --remove-orphans ok"
else
  report FAIL "[2/6]" "down -v falhou: $DOWN_OUT"
fi

# --- [3/6] up -d só backend (web fora: npm run dev ocupa :3000 no host) ---
UP_ARGS=(up -d); [[ "$DO_BUILD" == true ]] && UP_ARGS+=(--build)
UP_ARGS+=("${BACKEND_SERVICES[@]}"); [[ "$WITH_WEB" == true ]] && UP_ARGS+=(web)
if UP_OUT="$(STUDIO_PASSWORD="$STUDIO_PASSWORD" compose "${UP_ARGS[@]}" 2>&1)"; then
  report PASS "[3/6]" "up -d ok"
else
  report FAIL "[3/6]" "up -d falhou: $UP_OUT"
fi

# --- [4/6] Esperas (timeout total compartilhado) ---
deadline=$((SECONDS + TIMEOUT))
s3_ok=false
while [[ $SECONDS -lt $deadline ]]; do
  # Decisão de formato: `compose ps --format json` varia entre versões do
  # plugin (array único vs NDJSON); grep por "ExitCode":0 cobre os dois sem
  # parser frágil. Fallback: docker inspect State.Status/ExitCode do s3-init.
  if compose ps s3-init --format json 2>/dev/null | grep -q '"ExitCode":0'; then s3_ok=true; break; fi
  cid="$(docker ps -a --filter 'name=s3-init' --format '{{.ID}}' 2>/dev/null | head -n1 || true)"
  if [[ -n "${cid:-}" ]] && [[ "$(docker inspect --format '{{.State.Status}}:{{.State.ExitCode}}' "$cid" 2>/dev/null)" == "exited:0" ]]; then s3_ok=true; break; fi
  sleep 3
done
if [[ "$s3_ok" == true ]]; then
  report PASS "[4/6]" "s3-init completed (exit 0)"
else
  report FAIL "[4/6]" "s3-init não completou em ${TIMEOUT}s"
fi
health_ok=false
while [[ $SECONDS -lt $deadline ]]; do
  if curl -fsS "$API/health" 2>/dev/null | grep -q '"auth"[[:space:]]*:[[:space:]]*"ready"'; then health_ok=true; break; fi
  sleep 3
done
if [[ "$health_ok" == true ]]; then
  report PASS "[4/6]" "/health auth=ready"
else
  report FAIL "[4/6]" "/health sem auth=ready em ${TIMEOUT}s"
fi

# --- [5/6] ASSERTS day-one ---
if curl -fsS "$API/health" 2>/dev/null | grep -q '"auth"[[:space:]]*:[[:space:]]*"ready"'; then
  report PASS "[5/6]" "(1) GET /health auth=ready"
else
  report FAIL "[5/6]" "(1) GET /health sem auth=ready"
fi
if bash "$ROOT_DIR/infra/scripts/ensure-bucket.sh" >/dev/null 2>&1; then
  report PASS "[5/6]" "(2) bucket garantido (ensure-bucket.sh exit 0)"
else
  report FAIL "[5/6]" "(2) ensure-bucket.sh exit != 0"
fi
# (3) login com a senha do shell; se falhar, fallback de autonomia: o
# principal (S1) loga em JSON o campo estruturado bootstrap_password = 32hex
# UMA vez quando STUDIO_PASSWORD ausente + users vazia (main.rs); via compose
# com env definida esse ramo não dispara — aí o 200 do shell é o caminho real.
# Extrator casa o campo JSON exato (o texto livre "gerada (copie…" quebraria).
LOGIN_CODE="$(curl -sS -o /dev/null -w '%{http_code}' -X POST "$API/api/auth/login" -H 'Content-Type: application/json' -d "{\"password\":\"$STUDIO_PASSWORD\"}" 2>/dev/null || echo 000)"
if [[ "$LOGIN_CODE" == "200" ]]; then
  report PASS "[5/6]" "(3) login 200 com STUDIO_PASSWORD do shell"
else
  EXTRACTED="$(compose logs principal 2>/dev/null | grep -oE '"bootstrap_password":"[0-9a-f]{32}"' | tail -n1 | grep -oE '[0-9a-f]{32}' || true)"
  RETRY="000"
  [[ -n "${EXTRACTED:-}" ]] && RETRY="$(curl -sS -o /dev/null -w '%{http_code}' -X POST "$API/api/auth/login" -H 'Content-Type: application/json' -d "{\"password\":\"$EXTRACTED\"}" 2>/dev/null || echo 000)"
  if [[ "$RETRY" == "200" ]]; then
    report PASS "[5/6]" "(3) login 200 com senha de autonomia dos logs"
  else
    report FAIL "[5/6]" "(3) login falhou (shell=$LOGIN_CODE, autonomia=$RETRY)"
  fi
fi
# (4) fail-fast honesto: restarting/restart-loop OU Exited (1..) com
# restart:"no" (morto por erro — o grep antigo dava PASS espúrio). s3-init
# Exited (0) é o estado ESPERADO e não casa em \([1-9]; Status incluído no
# formato porque .State sozinho ("exited") não distingue o exit code.
BAD="$(compose ps --format '{{.Name}} {{.State}} {{.Status}}' 2>/dev/null | grep -E 'restarting|restart-loop|Exited \([1-9]' || true)"
if [[ -z "$BAD" ]]; then
  report PASS "[5/6]" "(4) nenhum serviço em restart-loop"
else
  report FAIL "[5/6]" "(4) restart-loop: $BAD"
fi
if [[ "$SKIP_SMOKE" == true ]]; then
  report PASS "[5/6]" "(5) smoke pulado (--skip-smoke)"
else
  # Decisão: e2e-smoke.sh aceita UM modo por chamada; roda --auth (contrato
  # auth) e depois --datasets (storage) em sequência como gate final.
  SMOKE_OK=true
  STUDIO_PASSWORD="$STUDIO_PASSWORD" bash "$ROOT_DIR/scripts/e2e-smoke.sh" --auth >/dev/null 2>&1 || SMOKE_OK=false
  if [[ "$SMOKE_OK" == true ]]; then
    STUDIO_PASSWORD="$STUDIO_PASSWORD" bash "$ROOT_DIR/scripts/e2e-smoke.sh" --datasets >/dev/null 2>&1 || SMOKE_OK=false
  fi
  if [[ "$SMOKE_OK" == true ]]; then
    report PASS "[5/6]" "(5) smoke e2e --auth + --datasets"
  else
    report FAIL "[5/6]" "(5) smoke e2e falhou"
  fi
fi

# --- [6/6] Resumo ---
echo "==== reset-dev: resumo ===="
printf '%s\n' "${RESULTS[@]}"
echo "PASS=$PASS_COUNT FAIL=$FAIL_COUNT"
[[ "$GENERATED" == true ]] && echo "STUDIO_PASSWORD gerada (p/ testar login): $STUDIO_PASSWORD"
[[ "$FAIL_COUNT" -gt 0 ]] && echo "Diagnóstico: docker compose -f $COMPOSE_FILE logs --tail=50 principal manager orchestrator-local seaweedfs s3-init db"
[[ "$FAIL_COUNT" -eq 0 ]]
