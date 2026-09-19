#!/usr/bin/env bash
# ==============================================================================
# scripts/install.sh — Instalador One-Line do Hephaestus LLM Studio
# ==============================================================================
# Baixa e configura o Hephaestus Studio diretamente via curl sem necessidade
# de clonar o repositório git inteiro manualmente.
#
# Uso:
#   curl -fsSL https://raw.githubusercontent.com/felipecaninnovaes/Hephaestus-LLM-Studio/main/scripts/install.sh | bash
#   curl -fsSL https://raw.githubusercontent.com/felipecaninnovaes/Hephaestus-LLM-Studio/main/scripts/install.sh | bash -s -- --auto
#   curl -fsSL https://raw.githubusercontent.com/felipecaninnovaes/Hephaestus-LLM-Studio/main/scripts/install.sh | bash -s -- --auto --start
# ==============================================================================

set -euo pipefail

REPO_OWNER="felipecaninnovaes"
REPO_NAME="Hephaestus-LLM-Studio"
DEFAULT_BRANCH="main"
BRANCH="${HEPH_BRANCH:-$DEFAULT_BRANCH}"
TARGET_DIR="${HEPH_DIR:-$HOME/hephaestus-studio}"
AUTO_MODE=0
START_NOW=0

usage() {
    echo "Hephaestus LLM Studio — Instalador One-Line"
    echo ""
    echo "Uso: curl -fsSL .../install.sh | bash -s -- [opções]"
    echo ""
    echo "Opções:"
    echo "  --dir <caminho>   Diretório de instalação (padrão: ~/hephaestus-studio)"
    echo "  --branch <nome>   Branch ou tag a baixar (padrão: main)"
    echo "  --auto            Modo não-interativo (gera segredos e detecta GPU)"
    echo "  --start           Inicia os containers automaticamente após o setup"
    echo "  -h, --help        Exibe esta mensagem de ajuda"
    echo ""
    exit 0
}

# Parse de argumentos
while [[ $# -gt 0 ]]; do
    case "$1" in
        --dir)
            TARGET_DIR="$2"
            shift 2
            ;;
        --branch)
            BRANCH="$2"
            shift 2
            ;;
        --auto)
            AUTO_MODE=1
            shift
            ;;
        --start)
            START_NOW=1
            shift
            ;;
        -h|--help)
            usage
            ;;
        *)
            echo "Aviso: opção desconhecida ignorada: $1" >&2
            shift
            ;;
    esac
done

echo "================================================================="
echo "   Hephaestus LLM Studio — Instalador Automatizado               "
echo "================================================================="
echo ""

# 1. Checagem de Dependências Básicas
echo "[1/4] Verificando dependências do sistema..."
MISSING_DEPS=()

for dep in curl tar openssl; do
    if ! command -v "$dep" >/dev/null 2>&1; then
        MISSING_DEPS+=("$dep")
    fi
done

if ! command -v docker >/dev/null 2>&1; then
    MISSING_DEPS+=("docker")
fi

if [ ${#MISSING_DEPS[@]} -gt 0 ]; then
    echo ""
    echo "ERRO: Dependências ausentes no sistema: ${MISSING_DEPS[*]}" >&2
    echo "Por favor, instale os pacotes necessários antes de continuar:" >&2
    echo "  Ubuntu/Debian: sudo apt update && sudo apt install -y curl tar openssl docker.io docker-compose-v2" >&2
    echo "  Arch Linux:    sudo pacman -S curl tar openssl docker docker-compose" >&2
    echo "  CentOS/RHEL:   sudo dnf install -y curl tar openssl docker-ce docker-compose-plugin" >&2
    exit 1
fi

# Verifica suporte a docker compose
DOCKER_COMPOSE_CMD=""
if docker compose version >/dev/null 2>&1; then
    DOCKER_COMPOSE_CMD="docker compose"
elif command -v docker-compose >/dev/null 2>&1; then
    DOCKER_COMPOSE_CMD="docker-compose"
else
    echo "ERRO: Docker Compose (v2 ou v1) não foi encontrado no PATH." >&2
    exit 1
fi
echo "[✓] Dependências verificadas com sucesso ($DOCKER_COMPOSE_CMD disponível)."

# 2. Download e Extração do Pacote de Deploy
echo ""
echo "[2/4] Baixando arquivos de deploy do Hephaestus ($BRANCH) em: $TARGET_DIR"
mkdir -p "$TARGET_DIR"

TARBALL_URL="https://github.com/${REPO_OWNER}/${REPO_NAME}/archive/refs/heads/${BRANCH}.tar.gz"

if ! curl -fSL --progress-bar "$TARBALL_URL" | tar -xz --strip-components=1 -C "$TARGET_DIR"; then
    echo "Falha ao baixar branch '${BRANCH}'. Tentando tag de release mais recente..."
    RELEASE_URL="https://github.com/${REPO_OWNER}/${REPO_NAME}/archive/refs/tags/latest.tar.gz"
    curl -fSL "$RELEASE_URL" | tar -xz --strip-components=1 -C "$TARGET_DIR"
fi

# Ajusta permissões de execução dos scripts
chmod +x "$TARGET_DIR"/scripts/*.sh "$TARGET_DIR"/compose/*.sh 2>/dev/null || true
echo "[✓] Arquivos instalados com sucesso."

# 3. Execução do Assistente de Configuração
echo ""
echo "[3/4] Inicializando configurações e credenciais..."
cd "$TARGET_DIR"

SETUP_ARGS=()
if [ "$AUTO_MODE" -eq 1 ]; then
    SETUP_ARGS+=("--auto")
fi

./scripts/setup.sh "${SETUP_ARGS[@]}"

# 4. Inicialização Automática Opcional
if [ "$START_NOW" -eq 1 ]; then
    echo ""
    echo "[4/4] Iniciando containers via $DOCKER_COMPOSE_CMD..."
    
    # Determina o perfil recomendado a partir do .env gerado
    PROFILE_FILE="compose/local-sem-node.yaml"
    if [ -f "$TARGET_DIR/compose/.env" ]; then
        if grep -q "local-com-local-node-gpu.yaml" "$TARGET_DIR/compose/.env" 2>/dev/null; then
            PROFILE_FILE="compose/local-com-local-node-gpu.yaml"
        elif grep -q "local-sem-node.yaml" "$TARGET_DIR/compose/.env" 2>/dev/null; then
            PROFILE_FILE="compose/local-sem-node.yaml"
        fi
    fi
    
    $DOCKER_COMPOSE_CMD -f "$PROFILE_FILE" up -d
    echo ""
    echo "[✓] Containers iniciados com sucesso!"
fi
