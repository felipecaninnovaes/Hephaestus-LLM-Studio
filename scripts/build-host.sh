#!/usr/bin/env bash
# ==============================================================================
# build-host.sh — Constrói todas as imagens Docker do host de desenvolvimento
# ==============================================================================
# Uso:
#   ./scripts/build-host.sh                     # Constrói todas as imagens locais
#   ./scripts/build-host.sh trainer-difusao     # Constrói apenas o trainer de difusão mock
#   ./scripts/build-host.sh principal manager   # Constrói apenas serviços específicos
#
# Não requer sudo (executa via grupo docker padrão).
# ==============================================================================
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
COMPOSE_FILE="$ROOT_DIR/infra/compose.yaml"

# Paleta de cores para saída legível
BOLD='\033[1m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[0;33m'
RED='\033[0;31m'
NC='\033[0m' # No Color

echo -e "${BOLD}${BLUE}=== [Hephaestus Studio] Build de Imagens do Host (Local / CPU) ===${NC}"
echo -e "Diretório base: ${ROOT_DIR}"
echo -e "Arquivo compose: ${COMPOSE_FILE}"

if ! command -v docker >/dev/null 2>&1; then
    echo -e "${RED}[ERRO] O comando 'docker' não foi encontrado no PATH.${NC}" >&2
    exit 1
fi

# Mapeamento de atalhos amigáveis para nomes de serviços no compose.yaml
TARGETS=()
for arg in "$@"; do
    case "$arg" in
        difusao|diffusion)
            TARGETS+=("trainer-difusao")
            ;;
        yolo)
            TARGETS+=("trainer-yolo")
            ;;
        clip|embedder)
            TARGETS+=("embedder")
            ;;
        orchestrator)
            TARGETS+=("orchestrator-local")
            ;;
        *)
            TARGETS+=("$arg")
            ;;
    esac
done

if [ ${#TARGETS[@]} -eq 0 ]; then
    echo -e "\n${YELLOW}Construindo todas as imagens de desenvolvimento (serviços + profiles de build)...${NC}"
    echo -e "Imagens inclusas: principal, manager, orchestrator-local, embedder, web, trainer-yolo, trainer-difusao\n"
    docker compose -f "$COMPOSE_FILE" --profile build build
else
    echo -e "\n${YELLOW}Construindo serviços específicos: ${TARGETS[*]}...${NC}\n"
    docker compose -f "$COMPOSE_FILE" --profile build build "${TARGETS[@]}"
fi

echo -e "\n${GREEN}${BOLD}✓ Build de imagens do host concluído com sucesso!${NC}\n"
