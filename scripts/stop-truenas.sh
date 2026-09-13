#!/usr/bin/env bash
# ==============================================================================
# Hephaestus LLM Studio — Para o Ambiente GPU no TrueNAS
#
# Pode ser executado:
#   1. Diretamente no TrueNAS (via terminal/SSH)
#   2. A partir do Host de Desenvolvimento (conecta automaticamente via SSH)
#
# Uso:
#   ./scripts/stop-truenas.sh          # Para e remove os containers da GPU
#   ./scripts/stop-truenas.sh --local  # Força execução local sem disparar SSH
# ==============================================================================

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
COMPOSE_FILE="$ROOT_DIR/infra/compose.gpu.yaml"
TRUENAS_HOST="${TRUENAS_HOST:-10.15.1.2}"
TRUENAS_USER="${TRUENAS_USER:-dockeruser}"

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
  echo "=== [Hephaestus Studio] Parando Nó GPU no TrueNAS via SSH ==="
  echo "Alvo: $TRUENAS_USER@$TRUENAS_HOST"
  echo ""
  exec ssh -o StrictHostKeyChecking=no "$TRUENAS_USER@$TRUENAS_HOST" \
    "cd ~/Hephaestus-LLM-Studio && ./scripts/stop-truenas.sh --local"
fi

echo "=== [Hephaestus Studio] Parando Ambiente GPU (TrueNAS) ==="
docker compose -p gpu -f "$COMPOSE_FILE" down --remove-orphans
echo "✓ Ambiente GPU parado com sucesso."
