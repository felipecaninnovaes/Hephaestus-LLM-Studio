#!/usr/bin/env bash
# ==============================================================================
# build-all.sh — Entrypoint unificado de compilação de imagens Docker
# ==============================================================================
# Uso:
#   ./scripts/build-all.sh host          # Constrói todas as imagens locais do host
#   ./scripts/build-all.sh gpu           # Constrói todas as imagens GPU
#   ./scripts/build-all.sh all           # Constrói host e GPU
#   ./scripts/build-all.sh               # Auto-detecta o ambiente (host vs GPU)
#
# Não requer sudo (executa via grupo docker padrão).
# ==============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

BOLD='\033[1m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[0;33m'
RED='\033[0;31m'
NC='\033[0m'

MODE="${1:-}"

case "$MODE" in
    host|local|cpu)
        shift || true
        bash "$SCRIPT_DIR/build-host.sh" "$@"
        ;;
    gpu|remote)
        shift || true
        bash "$SCRIPT_DIR/build-gpu.sh" "$@"
        ;;
    all)
        shift || true
        echo -e "${BOLD}${BLUE}>>> 1/2: Compilando imagens do Host local...${NC}"
        bash "$SCRIPT_DIR/build-host.sh" "$@"
        echo -e "${BOLD}${BLUE}>>> 2/2: Compilando imagens da GPU...${NC}"
        bash "$SCRIPT_DIR/build-gpu.sh" "$@"
        echo -e "${GREEN}${BOLD}✓ Todas as imagens (Host + GPU) foram compiladas com sucesso!${NC}"
        ;;
    "")
        # Auto-detecção inteligente: se nvidia-smi estiver presente e funcionando, sugere GPU
        if command -v nvidia-smi >/dev/null 2>&1 && nvidia-smi -L >/dev/null 2>&1; then
            echo -e "${YELLOW}[Auto-detecção] GPU NVIDIA detectada neste host.${NC}"
            echo -e "Executando build do ambiente GPU...\n"
            bash "$SCRIPT_DIR/build-gpu.sh"
        else
            echo -e "${BLUE}[Auto-detecção] Nenhuma GPU detectada neste host (modo CPU/dev).${NC}"
            echo -e "Executando build do ambiente Host...\n"
            bash "$SCRIPT_DIR/build-host.sh"
        fi
        ;;
    *)
        echo -e "${RED}[ERRO] Modo desconhecido: '$MODE'${NC}"
        echo -e "Uso:"
        echo -e "  $0 host     # Constrói imagens do dev host (CPU / local)"
        echo -e "  $0 gpu      # Constrói imagens do servidor GPU (TrueNAS)"
        echo -e "  $0 all      # Constrói ambos"
        exit 1
        ;;
esac
