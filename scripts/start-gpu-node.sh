#!/usr/bin/env bash
# ==============================================================================
# Hephaestus LLM Studio — Inicia o Ambiente GPU no Nó GPU Dedicado
#
# Pode ser executado:
#   1. Diretamente no nó GPU (via terminal/SSH)
#   2. A partir do Host de Desenvolvimento (conecta automaticamente via SSH)
#
# Uso:
#   ./scripts/start-gpu-node.sh          # Inicia o container orchestrator-gpu
#   ./scripts/start-gpu-node.sh --build  # Reconstrói a imagem antes de subir
#   ./scripts/start-gpu-node.sh --logs   # Acompanha logs do orchestrator-gpu
#   ./scripts/start-gpu-node.sh --local  # Força execução local sem disparar SSH
# ==============================================================================

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
COMPOSE_FILE="$ROOT_DIR/infra/compose.gpu.yaml"
ENV_FILE="$ROOT_DIR/infra/env.gpu"
GPU_NODE_HOST="${GPU_NODE_HOST:-10.15.50.114}"
GPU_NODE_USER="${GPU_NODE_USER:-dockeruser}"

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

# Detecta se estamos no nó GPU ou se devemos conectar via SSH.
# Sem heurística de vendor (não é TrueNAS) — detecção por hostname `docker-04`,
# IP local batendo com GPU_NODE_HOST, ou flag explícita --local.
is_on_gpu_node() {
  if [[ "$IS_LOCAL" == true ]]; then
    return 0
  fi
  if [[ "$(hostname)" == "docker-04" ]]; then
    return 0
  fi
  if ip addr 2>/dev/null | grep -q "$GPU_NODE_HOST"; then
    return 0
  fi
  return 1
}

if ! is_on_gpu_node; then
  echo "=== [Hephaestus Studio] Disparando Inicialização no Nó GPU via SSH ==="
  echo "Alvo: $GPU_NODE_USER@$GPU_NODE_HOST"
  echo ""
  exec ssh -o StrictHostKeyChecking=no "$GPU_NODE_USER@$GPU_NODE_HOST" \
    "cd ~/Hephaestus-LLM-Studio && ./scripts/start-gpu-node.sh --local ${FORWARD_ARGS[*]:-}"
fi

# Execução dentro do nó GPU
echo "=== [Hephaestus Studio] Iniciando Ambiente GPU (Nó Dedicado) ==="
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
  echo "[AVISO] Disco do nó é limitado (~54GB livres) — evite buildar tudo de uma vez."
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
echo "  • Orquestrador GPU : http://$GPU_NODE_HOST:8082"
echo "  • Heartbeat Manager: configurado no env.gpu (MANAGER_URL)"
echo "  • Storage S3       : configurado no env.gpu (S3_ORCH_ENDPOINT_URL)"

if [[ "$FOLLOW_LOGS" == true ]]; then
  echo ""
  echo "=== Acompanhando Logs do orchestrator-gpu (Ctrl+C para sair) ==="
  "${COMPOSE_CMD[@]}" logs -f orchestrator-gpu
fi
