#!/usr/bin/env bash
# ==============================================================================
# Hephaestus LLM Studio — Roda o api-principal NATIVO no host (dev / iteração)
#
# Para iteração rápida em Rust sem rebuild de imagem: executa o binário
# ./target/release/api-principal em FOREGROUND, com env correto para rede do
# host (localhost, não DNS de container) e fail-fast honesto.
#
# Uso:
#   STUDIO_PASSWORD='...' ./scripts/run-native.sh          # roda o binário
#   STUDIO_PASSWORD='...' ./scripts/run-native.sh --build  # build + roda
#   ./scripts/run-native.sh --help
# ==============================================================================

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
COMPOSE_FILE="$ROOT_DIR/infra/compose.yaml"
BIN="$ROOT_DIR/target/release/api-principal"

DO_BUILD=false

# Parser de argumentos
while [[ $# -gt 0 ]]; do
  case "$1" in
    --build|-b)
      DO_BUILD=true
      shift
      ;;
    --help|-h)
      echo "Uso: STUDIO_PASSWORD='...' $0 [--build]"
      echo ""
      echo "Roda o api-principal nativo no host em foreground (logs no terminal)."
      echo ""
      echo "Opções:"
      echo "  --build, -b   Roda 'cargo build --release -p api-principal' antes"
      echo "  --help, -h    Exibe esta ajuda"
      echo ""
      echo "Env:"
      echo "  STUDIO_PASSWORD  Obrigatória (sem default silencioso)"
      echo "  DATABASE_URL     Default postgres://studio:studio@localhost:5432/studio"
      echo "  MANAGER_URL      Default http://localhost:8081 (rede do host)"
      echo "  MANAGER_TOKEN    Default changeme (dev local; avisa se usar)"
      echo "  ENGINE_MOCK      Default 1 (CPU-only no dev; respeita exportado)"
      echo ""
      echo "Requer Linux + bash + coreutils (timeout)."
      exit 0
      ;;
    *)
      echo "Opção desconhecida: $1" >&2
      echo "Use '$0 --help' para ver as opções disponíveis." >&2
      exit 1
      ;;
  esac
done

# STUDIO_PASSWORD obrigatória: bootstrap do usuário único acontece só no 1º
# boot com tabela users vazia — default silencioso cravaria 'changeme' sem
# alerta e travaria o login depois. Falhar cedo e alto é honesto.
if [[ -z "${STUDIO_PASSWORD:-}" ]]; then
  echo "Erro: STUDIO_PASSWORD não definida." >&2
  echo "Receita: STUDIO_PASSWORD='...' ./scripts/run-native.sh" >&2
  exit 1
fi

# Defaults pensados para rede do HOST (localhost), não DNS de container.
# Todos sobrescrevíveis por env exportado antes da chamada.
export DATABASE_URL="${DATABASE_URL:-postgres://studio:studio@localhost:5432/studio}"
export MANAGER_URL="${MANAGER_URL:-http://localhost:8081}"
if [[ -z "${MANAGER_TOKEN:-}" ]]; then
  export MANAGER_TOKEN="changeme"
  echo "Aviso: MANAGER_TOKEN usando default 'changeme' (dev local)." >&2
fi
# CPU-only é inegociável no dev; respeita valor exportado pelo usuário.
export ENGINE_MOCK="${ENGINE_MOCK:-1}"

# URL nunca logada: contém credencial do Postgres (padrão main.rs:105-107).
# Versão mascarada p/ banner e erros; o export real acima permanece intacto.
DATABASE_URL_MASKED="$(printf '%s' "$DATABASE_URL" | sed -E 's|^([a-z+]+://)[^@]*@|\1***@|')"

echo "=== [Hephaestus Studio] api-principal nativo ==="
echo "DATABASE_URL : $DATABASE_URL_MASKED"
echo "MANAGER_URL  : $MANAGER_URL"
echo "ENGINE_MOCK  : $ENGINE_MOCK"

# Preflight 1: Postgres alcançável. Método portável sem psql/pg_isready/nc no
# host: probe TCP via /dev/tcp (builtin do bash) no host/porta extraídos da
# DATABASE_URL. docker exec serviria, mas acopla ao nome do container.
# TCP-only: URL sem '@' (unix-socket ou formato exótico) não dá host/porta —
# o probe mentiria; falha cedo em vez de testar o destino errado.
if [[ "$DATABASE_URL" != *@* ]]; then
  echo "Erro: DATABASE_URL sem '@' (unix-socket?) não suportada; use TCP postgres://user:pass@host:port/db." >&2
  exit 1
fi
db_rest="${DATABASE_URL##*@}"
db_hostport="${db_rest%%/*}"
DB_HOST="${db_hostport%%:*}"
DB_PORT="${db_hostport##*:}"
[[ "$DB_HOST" == "$db_hostport" ]] && DB_PORT="5432"
if ! timeout 3 bash -c "</dev/tcp/$DB_HOST/$DB_PORT" >/dev/null 2>&1; then
  echo "Erro: Postgres inalcançável em $DB_HOST:$DB_PORT (de $DATABASE_URL_MASKED)." >&2
  echo "Suba o banco primeiro (ex.: docker compose -f $COMPOSE_FILE up -d db)." >&2
  exit 1
fi

# Preflight 2: porta 8080 livre. O conflito 8080:8080 com o container compose
# é armadilha real da casa: nativo e container não dividem a porta.
if (echo > /dev/tcp/127.0.0.1/8080) >/dev/null 2>&1; then
  if command -v docker >/dev/null 2>&1 && docker ps --format '{{.Names}}' 2>/dev/null | grep -q '^infra-principal-1$'; then
    echo "Erro: porta 8080 ocupada pelo container infra-principal-1." >&2
    echo "Sugestão: docker compose -f $COMPOSE_FILE stop principal" >&2
  else
    echo "Erro: porta 8080 já ocupada por processo no host. Encerre-o antes." >&2
  fi
  exit 1
fi

# Preflight 3: bucket S3 heph-data (WARN-ONLY, nunca bloqueia o boot).
# `docker compose down -v` apaga o volume seaweed_data e o bucket some junto
# (a identidade volta via infra/seaweedfs-s3.json, mas o bucket não é
# auto-criado p/ identidade escopada) — uploads de artefatos falham então
# silenciosamente (jobs "done" com zero artefatos). Tenta garantir via
# infra/scripts/ensure-bucket.sh (SigV4 artesanal, só curl+openssl) com env
# inline (defaults localhost:8333 + creds LOCAL-DEV do compose; respeita
# S3_* exportado). Falha ou ferramentas ausentes = WARN, o binário continua.
S3_PREFLIGHT_OUT=""
if command -v curl >/dev/null 2>&1 && command -v openssl >/dev/null 2>&1; then
  S3_PREFLIGHT_OUT="$(S3_ENDPOINT_URL="${S3_ENDPOINT_URL:-http://localhost:8333}" \
    S3_BUCKET="${S3_BUCKET:-heph-data}" \
    S3_ACCESS_KEY="${S3_ACCESS_KEY:-heph}" \
    S3_SECRET_KEY="${S3_SECRET_KEY:-heph-local-dev}" \
    sh "$ROOT_DIR/infra/scripts/ensure-bucket.sh" 2>&1)" || {
    echo "Aviso: bucket S3 '${S3_BUCKET:-heph-data}' não garantido — ${S3_PREFLIGHT_OUT:-falha desconhecida}" >&2
    echo "Aviso: uploads de artefatos (jobs) vão falhar; suba o seaweedfs e rode: sh $ROOT_DIR/infra/scripts/ensure-bucket.sh" >&2
  }
else
  echo "Aviso: sem curl/openssl no host — bucket S3 '${S3_BUCKET:-heph-data}' não verificado (uploads de artefatos podem falhar)." >&2
fi

if [[ "$DO_BUILD" == true ]]; then
  echo "Compilando api-principal (release)..."
  cargo build --release -p api-principal --manifest-path "$ROOT_DIR/Cargo.toml"
fi

if [[ ! -x "$BIN" ]]; then
  echo "Erro: binário não encontrado em $BIN." >&2
  echo "Rode com --build ou compile manualmente: cargo build --release -p api-principal" >&2
  exit 1
fi

echo "Executando $BIN em foreground (Ctrl+C para encerrar)..."
exec "$BIN"
