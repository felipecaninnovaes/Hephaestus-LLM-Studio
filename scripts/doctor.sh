#!/usr/bin/env bash
# ==============================================================================
# scripts/doctor.sh — Diagnóstico Pré-Voo do Hephaestus LLM Studio
# ==============================================================================
# Inspeciona requisitos de sistema, dependências, portas e aceleração GPU.
# Uso: bash scripts/doctor.sh
# ==============================================================================
set -u

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

ERRORS=0
WARNINGS=0

check_ok() {
    echo -e "  [${GREEN}✓${NC}] $1"
}

check_warn() {
    echo -e "  [${YELLOW}!${NC}] $1"
    WARNINGS=$((WARNINGS + 1))
}

check_err() {
    echo -e "  [${RED}✗${NC}] $1"
    ERRORS=$((ERRORS + 1))
}

echo -e "\n${BLUE}================================================================${NC}"
echo -e "${BLUE}       HEPHAESTUS LLM STUDIO — DIAGNÓSTICO PRÉ-VOO              ${NC}"
echo -e "${BLUE}================================================================${NC}\n"

# 1. Ferramentas Básicas de CLI
echo "1. Ferramentas de Sistema:"
for cmd in curl openssl jq git; do
    if command -v "$cmd" >/dev/null 2>&1; then
        check_ok "Comando '$cmd' instalado ($($cmd --version 2>/dev/null | head -n1 || echo 'ok'))"
    else
        check_err "Comando '$cmd' NÃO encontrado (instale via gerenciador de pacotes)"
    fi
done

# 2. Runtimes (Rust, Node, Python, uv)
echo -e "\n2. Runtimes de Desenvolvimento:"
if command -v cargo >/dev/null 2>&1; then
    check_ok "Rust / Cargo: $(cargo --version)"
else
    check_warn "Cargo não instalado (necessário para compilação nativa dos serviços Rust)"
fi

if command -v node >/dev/null 2>&1; then
    check_ok "Node.js: $(node --version)"
else
    check_warn "Node.js não instalado (necessário para build da UI web)"
fi

if command -v python3 >/dev/null 2>&1; then
    check_ok "Python: $(python3 --version)"
else
    check_warn "Python 3 não instalado"
fi

if command -v uv >/dev/null 2>&1; then
    check_ok "uv (gerenciador Python): $(uv --version)"
else
    check_warn "uv não instalado (recomendado para gerenciar ambientes das engines)"
fi

# 3. Docker e Compose
echo -e "\n3. Docker e Orquestração:"
if command -v docker >/dev/null 2>&1; then
    check_ok "Docker CLI: $(docker --version)"
    if docker info >/dev/null 2>&1; then
        check_ok "Docker daemon está rodando e acessível"
    else
        check_err "Docker daemon NÃO está rodando ou permissão negada no socket"
    fi
else
    check_err "Docker NÃO instalado"
fi

if docker compose version >/dev/null 2>&1; then
    check_ok "Docker Compose: $(docker compose version)"
else
    check_err "Docker Compose plugin NÃO encontrado"
fi

# 4. Checagem de Conflitos de Portas no Host
echo -e "\n4. Portas de Rede:"
check_port() {
    local port=$1
    local name=$2
    if command -v nc >/dev/null 2>&1; then
        if nc -z 127.0.0.1 "$port" 2>/dev/null; then
            check_warn "Porta $port ($name) já está em uso no host"
            return
        fi
    fi
    check_ok "Porta $port ($name) livre"
}

check_port 3000 "Web Studio UI"
check_port 8080 "API Principal (BFF)"
check_port 8081 "Manager"
check_port 8082 "Orchestrator"
check_port 8333 "SeaweedFS S3"
check_port 5432 "PostgreSQL (pgvector)"

# 5. Aceleração por Hardware / GPU
echo -e "\n5. Hardware & GPU:"
if command -v nvidia-smi >/dev/null 2>&1; then
    GPU_NAME=$(nvidia-smi --query-gpu=name --format=csv,noheader 2>/dev/null | head -n1 || echo "NVIDIA")
    check_ok "NVIDIA GPU detectada: $GPU_NAME"
    if docker run --rm --gpus all nvidia/cuda:12.4.0-base-ubuntu22.04 nvidia-smi >/dev/null 2>&1; then
        check_ok "Docker NVIDIA Container Toolkit operacional (--gpus all ok)"
    else
        check_warn "NVIDIA GPU presente, mas Docker NVIDIA Toolkit pode não estar configurado"
    fi
else
    check_ok "Nenhuma GPU NVIDIA proprietária (ambiente CPU-only / ENGINE_MOCK=1)"
fi

# 6. Espaço em Disco
echo -e "\n6. Armazenamento:"
FREE_GB=$(df -BG . | tail -n1 | awk '{print $4}' | tr -d 'G')
if [ "$FREE_GB" -gt 20 ]; then
    check_ok "Espaço livre em disco: ${FREE_GB} GB (>20 GB recomendado)"
elif [ "$FREE_GB" -gt 5 ]; then
    check_warn "Espaço livre em disco moderado: ${FREE_GB} GB (recomendado >20 GB para modelos)"
else
    check_err "Espaço em disco crítico: ${FREE_GB} GB livres"
fi

echo -e "\n${BLUE}================================================================${NC}"
if [ "$ERRORS" -eq 0 ]; then
    echo -e "${GREEN}✓ DIAGNÓSTICO CONCLUÍDO: Ambiente pronto para execução! ($WARNINGS avisos)${NC}\n"
    exit 0
else
    echo -e "${RED}✗ DIAGNÓSTICO CONCLUÍDO: $ERRORS erro(s) crítico(s) encontrado(s).${NC}\n"
    exit 1
fi
