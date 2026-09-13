#!/usr/bin/env bash
# ==============================================================================
# Hephaestus LLM Studio — Para o Ambiente Host (Dev / Local)
#
# Uso:
#   ./scripts/stop-host.sh          # Para e remove os containers
#   ./scripts/stop-host.sh --clean  # Para e remove containers + volumes anônimos
# ==============================================================================

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
COMPOSE_FILE="$ROOT_DIR/infra/compose.yaml"

echo "=== [Hephaestus Studio] Parando Ambiente Host ==="
if [[ "${1:-}" == "--clean" ]]; then
  echo "Removendo containers e volumes transitórios..."
  docker compose -f "$COMPOSE_FILE" down -v --remove-orphans
else
  docker compose -f "$COMPOSE_FILE" down --remove-orphans
fi

echo "✓ Ambiente Host parado com sucesso."
