#!/usr/bin/env bash
# ==============================================================================
# Hephaestus LLM Studio — Para o Ambiente GPU no Nó GPU Dedicado
#
# Pode ser executado:
#   1. Diretamente no nó GPU (via terminal/SSH)
#   2. A partir do Host de Desenvolvimento (conecta automaticamente via SSH)
#
# Uso:
#   ./scripts/stop-gpu-node.sh          # Para e remove os containers da GPU
#   ./scripts/stop-gpu-node.sh --local  # Força execução local sem disparar SSH
# ==============================================================================

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
COMPOSE_FILE="$ROOT_DIR/infra/compose.gpu.yaml"
GPU_NODE_HOST="${GPU_NODE_HOST:-10.15.50.114}"
GPU_NODE_USER="${GPU_NODE_USER:-dockeruser}"

IS_LOCAL=false

for arg in "$@"; do
  case "$arg" in
    --local)
      IS_LOCAL=true
      ;;
    --help|-h)
      echo "Uso: $0 [OPÇÕES]"
      echo ""
      echo "Opções:"
      echo "  --local        Executa localmente sem tentar conectar via SSH"
      echo "  --help, -h     Exibe esta ajuda"
      exit 0
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
  echo "=== [Hephaestus Studio] Parando Nó GPU Dedicado via SSH ==="
  echo "Alvo: $GPU_NODE_USER@$GPU_NODE_HOST"
  echo ""
  exec ssh -o StrictHostKeyChecking=no "$GPU_NODE_USER@$GPU_NODE_HOST" \
    "cd ~/Hephaestus-LLM-Studio && ./scripts/stop-gpu-node.sh --local"
fi

echo "=== [Hephaestus Studio] Parando Ambiente GPU (Nó Dedicado) ==="
docker compose -p gpu -f "$COMPOSE_FILE" down --remove-orphans
echo "✓ Ambiente GPU parado com sucesso."
