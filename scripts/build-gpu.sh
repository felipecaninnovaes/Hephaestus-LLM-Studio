#!/usr/bin/env bash
# ==============================================================================
# build-gpu.sh — Constrói todas as imagens Docker do ambiente GPU (TrueNAS / Servidor)
# ==============================================================================
# Uso:
#   ./scripts/build-gpu.sh                     # Constrói todas as imagens GPU
#   ./scripts/build-gpu.sh difusao             # Constrói apenas o trainer de difusão GPU
#   ./scripts/build-gpu.sh yolo                # Constrói apenas o trainer YOLO GPU
#   ./scripts/build-gpu.sh orchestrator        # Constrói o container do orquestrador GPU
#
# Não requer sudo (executa via grupo docker padrão).
# ==============================================================================
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
COMPOSE_FILE="$ROOT_DIR/infra/compose.gpu.yaml"
ENV_FILE="$ROOT_DIR/infra/env.gpu"

# Paleta de cores para saída legível
BOLD='\033[1m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[0;33m'
RED='\033[0;31m'
NC='\033[0m' # No Color

echo -e "${BOLD}${BLUE}=== [Hephaestus Studio] Build de Imagens GPU (TrueNAS / Remoto) ===${NC}"
echo -e "Diretório base: ${ROOT_DIR}"
echo -e "Arquivo compose: ${COMPOSE_FILE}"

if ! command -v docker >/dev/null 2>&1; then
    echo -e "${RED}[ERRO] O comando 'docker' não foi encontrado no PATH.${NC}" >&2
    exit 1
fi

# Prepara argumentos do docker compose
COMPOSE_ARGS=(-p gpu -f "$COMPOSE_FILE")
if [ -f "$ENV_FILE" ]; then
    COMPOSE_ARGS+=(--env-file "$ENV_FILE")
fi

# Mapeamento de atalhos amigáveis para nomes de serviços no compose.gpu.yaml
TARGETS=()
for arg in "$@"; do
    case "$arg" in
        difusao|diffusion|trainer-difusao)
            TARGETS+=("trainer-difusao-gpu")
            ;;
        yolo|trainer-yolo)
            TARGETS+=("trainer-gpu")
            ;;
        orchestrator|orch)
            TARGETS+=("orchestrator-gpu")
            ;;
        *)
            TARGETS+=("$arg")
            ;;
    esac
done

if [ ${#TARGETS[@]} -eq 0 ]; then
    echo -e "\n${YELLOW}Construindo todas as imagens GPU (orchestrator-gpu, trainer-gpu, trainer-difusao-gpu)...${NC}\n"
    docker compose "${COMPOSE_ARGS[@]}" --profile build build
else
    echo -e "\n${YELLOW}Construindo alvos específicos GPU: ${TARGETS[*]}...${NC}\n"
    docker compose "${COMPOSE_ARGS[@]}" --profile build build "${TARGETS[@]}"
fi

echo -e "\n${GREEN}${BOLD}✓ Build de imagens GPU concluído com sucesso!${NC}\n"
