#!/usr/bin/env bash
# ==============================================================================
# Hephaestus LLM Studio — Inicia o Ambiente GPU no TrueNAS
#
# Pode ser executado:
#   1. Diretamente no TrueNAS (via terminal/SSH)
#   2. A partir do Host de Desenvolvimento (conecta automaticamente via SSH)
#
# Uso:
#   ./scripts/start-truenas.sh          # Inicia o container orchestrator-gpu
#   ./scripts/start-truenas.sh --build  # Reconstrói a imagem antes de subir
#   ./scripts/start-truenas.sh --logs   # Acompanha logs do orchestrator-gpu
#   ./scripts/start-truenas.sh --local  # Força execução local sem disparar SSH
# ==============================================================================

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
COMPOSE_FILE="$ROOT_DIR/infra/compose.gpu.yaml"
ENV_FILE="$ROOT_DIR/infra/env.gpu"
TRUENAS_HOST="${TRUENAS_HOST:-10.15.1.2}"
TRUENAS_USER="${TRUENAS_USER:-dockeruser}"

IS_LOCAL=false
DO_BUILD=false
FOLLOW_LOGS=false
FORWARD_ARGS=()

for arg in "$@"; do
  case "$arg" in
    --local)
      IS_LOCAL=true
      ;;
    --build|-b)
      DO_BUILD=true
      FORWARD_ARGS+=("$arg")
      ;;
    --logs|-f|--follow)
      FOLLOW_LOGS=true
      FORWARD_ARGS+=("$arg")
      ;;
    --help|-h)
      echo "Uso: $0 [OPÇÕES]"
      echo ""
      echo "Opções:"
      echo "  --build, -b    Reconstrói o binário/imagem do orchestrator-gpu antes de subir"
      echo "  --logs, -f     Acompanha logs do orchestrator-gpu após inicialização"
      echo "  --local        Executa localmente sem tentar conectar via SSH"
      echo "  --help, -h     Exibe esta ajuda"
      exit 0
      ;;
    *)
      FORWARD_ARGS+=("$arg")
      ;;
  esac
done

# Detecta se estamos no TrueNAS ou se devemos conectar via SSH
is_on_truenas() {
  if [[ "$IS_LOCAL" == true ]]; then
    return 0
  fi
  if [[ -f /etc/truenas_version ]] || [[ "$(hostname)" =~ truenas ]]; then
    return 0
  fi
  if ip addr 2>/dev/null | grep -q "$TRUENAS_HOST"; then
    return 0
  fi
  return 1
}

if ! is_on_truenas; then
  echo "=== [Hephaestus Studio] Disparando Inicialização no TrueNAS via SSH ==="
  echo "Alvo: $TRUENAS_USER@$TRUENAS_HOST"
  echo ""
  exec ssh -o StrictHostKeyChecking=no "$TRUENAS_USER@$TRUENAS_HOST" \
    "cd ~/Hephaestus-LLM-Studio && ./scripts/start-truenas.sh --local ${FORWARD_ARGS[*]:-}"
fi

# Execução dentro do TrueNAS
echo "=== [Hephaestus Studio] Iniciando Ambiente GPU (TrueNAS) ==="
echo "Diretório base : $ROOT_DIR"
echo "Arquivo compose: $COMPOSE_FILE"
echo "Arquivo de env : $ENV_FILE"

if [[ ! -f "$ENV_FILE" ]]; then
  if [[ -f "$ROOT_DIR/infra/env.gpu.example" ]]; then
    echo "[AVISO] $ENV_FILE não encontrado. Criando a partir de env.gpu.example..."
    cp "$ROOT_DIR/infra/env.gpu.example" "$ENV_FILE"
  else
    echo "[ERRO] Arquivo de variáveis $ENV_FILE não encontrado!" >&2
    exit 1
  fi
fi

COMPOSE_CMD=("docker" "compose" "-p" "gpu" "--env-file" "$ENV_FILE" "-f" "$COMPOSE_FILE")

BUILD_FLAG=()
if [[ "$DO_BUILD" == true ]]; then
  echo "Reconstruindo imagens do orquestrador e trainers GPU (profile build)..."
  "${COMPOSE_CMD[@]}" --profile build build
  BUILD_FLAG+=("--build")
fi

echo "Subindo serviço orchestrator-gpu..."
"${COMPOSE_CMD[@]}" up -d "${BUILD_FLAG[@]}" orchestrator-gpu

echo ""
echo "=== Status do Nó GPU ==="
"${COMPOSE_CMD[@]}" ps orchestrator-gpu

echo ""
echo "=== Informações de Conexão do Nó ==="
echo "  • Orquestrador GPU : http://$TRUENAS_HOST:8082"
echo "  • Heartbeat Manager: configurado no env.gpu (MANAGER_URL)"
echo "  • Storage S3       : configurado no env.gpu (S3_ORCH_ENDPOINT_URL)"

if [[ "$FOLLOW_LOGS" == true ]]; then
  echo ""
  echo "=== Acompanhando Logs do orchestrator-gpu (Ctrl+C para sair) ==="
  "${COMPOSE_CMD[@]}" logs -f orchestrator-gpu
fi
