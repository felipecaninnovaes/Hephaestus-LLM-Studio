#!/usr/bin/env bash
# ==============================================================================
# Hephaestus LLM Studio — Inicia o Ambiente Host (Dev / Local)
#
# Modos de inicialização:
#   --no-web, dev   : Sobe os serviços backend no Docker (db, seaweedfs, embedder,
#                     principal, manager, orchestrator-local). O frontend Next.js
#                     deve ser executado localmente via `npm run dev` na porta 3000.
#                     (Modo padrão de desenvolvimento)
#
#   --with-web, full: Sobe TODOS os serviços no Docker, incluindo o container
#                     `web` (Next.js em produção na porta 3000).
#
# Uso:
#   ./scripts/start-host.sh              # Modo dev padrão (--no-web)
#   ./scripts/start-host.sh --no-web     # Modo dev (web fora do docker)
#   ./scripts/start-host.sh --with-web   # Modo full (web dentro do docker)
#   ./scripts/start-host.sh --build      # Reconstrói imagens antes de subir
#   ./scripts/start-host.sh --logs       # Acompanha logs dos containers
# ==============================================================================

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
COMPOSE_FILE="$ROOT_DIR/infra/compose.yaml"
ENV_FILE="$ROOT_DIR/infra/.env"

MODE="dev"
DO_BUILD=false
FOLLOW_LOGS=false

# Parser de argumentos
while [[ $# -gt 0 ]]; do
  case "$1" in
    --no-web|dev)
      MODE="dev"
      shift
      ;;
    --with-web|full)
      MODE="full"
      shift
      ;;
    --build|-b)
      DO_BUILD=true
      shift
      ;;
    --logs|-f|--follow)
      FOLLOW_LOGS=true
      shift
      ;;
    --help|-h)
      echo "Uso: $0 [OPÇÕES]"
      echo ""
      echo "Opções:"
      echo "  --no-web, dev    Inicia apenas o backend no Docker (Next.js roda local no host)"
      echo "  --with-web, full Inicia todo o ambiente no Docker, incluindo o container web"
      echo "  --build, -b      Força a reconstrução das imagens antes de iniciar"
      echo "  --logs, -f       Acompanha a saída de logs após a inicialização"
      echo "  --help, -h       Exibe esta ajuda"
      exit 0
      ;;
    *)
      echo "Opção desconhecida: $1" >&2
      echo "Use '$0 --help' para ver as opções disponíveis." >&2
      exit 1
      ;;
  esac
done

echo "=== [Hephaestus Studio] Iniciando Ambiente Host ==="
echo "Diretório raiz : $ROOT_DIR"
echo "Arquivo compose: $COMPOSE_FILE"
echo "Modo de execução: $MODE $([[ "$MODE" == "dev" ]] && echo "(Web fora do Docker)" || echo "(Web no Docker)")"
echo ""

COMPOSE_CMD=("docker" "compose" "-f" "$COMPOSE_FILE")
if [[ -f "$ENV_FILE" ]]; then
  COMPOSE_CMD+=("--env-file" "$ENV_FILE")
fi

BUILD_FLAG=()
if [[ "$DO_BUILD" == true ]]; then
  BUILD_FLAG+=("--build")
fi

if [[ "$MODE" == "full" ]]; then
  # Sobe todos os serviços (incluindo web)
  echo "Subindo todos os serviços no Docker (db, seaweedfs, embedder, principal, manager, orchestrator-local, web)..."
  "${COMPOSE_CMD[@]}" up -d "${BUILD_FLAG[@]}"
else
  # Sobe apenas backend no Docker, parando o container web se estiver rodando
  echo "Subindo serviços de backend no Docker (db, seaweedfs, embedder, principal, manager, orchestrator-local)..."
  "${COMPOSE_CMD[@]}" up -d "${BUILD_FLAG[@]}" db seaweedfs embedder principal manager orchestrator-local

  # Se o container web estiver ativo de uma execução anterior full, para ele para liberar a porta 3000
  if "${COMPOSE_CMD[@]}" ps --services --filter "status=running" 2>/dev/null | grep -q "^web$"; then
    echo "Parando container 'web' para liberar a porta 3000 para execução local..."
    "${COMPOSE_CMD[@]}" stop web
  fi
fi

echo ""
echo "=== Status dos Serviços ==="
"${COMPOSE_CMD[@]}" ps

echo ""
echo "=== Endpoints Disponíveis ==="
echo "  • API Principal       : http://localhost:8080"
echo "  • Manager de Nós      : http://localhost:8081"
echo "  • Orquestrador Local  : http://localhost:8082"
echo "  • Storage SeaweedFS S3: http://localhost:8333"
echo "  • Embedder CLIP       : http://localhost:8090"
echo "  • Banco Postgres      : localhost:5432 (db: studio)"

if [[ "$MODE" == "full" ]]; then
  echo "  • Interface Web (Docker): http://localhost:3000"
else
  echo ""
  echo "=== Frontend Web (Modo Dev) ==="
  echo "O backend está ativo no Docker. Para rodar o frontend Next.js com Hot Reload:"
  echo ""
  echo "  cd $ROOT_DIR/apps/web && npm run dev"
  echo ""
  echo "E acesse no navegador: http://localhost:3000"
fi

if [[ "$FOLLOW_LOGS" == true ]]; then
  echo ""
  echo "=== Acompanhando Logs (Ctrl+C para sair) ==="
  "${COMPOSE_CMD[@]}" logs -f
fi
