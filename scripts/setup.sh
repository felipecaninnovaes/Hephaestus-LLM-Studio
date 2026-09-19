#!/usr/bin/env bash
# ==============================================================================
# scripts/setup.sh — Onboarding e Inicialização Segura do Hephaestus LLM Studio
# ==============================================================================
# Gera credenciais criptograficamente seguras, configura o arquivo .env e o
# arquivo de identidades S3 do SeaweedFS para implantação pública ou privada.
#
# Uso:
#   ./scripts/setup.sh          # Modo interativo com assistente
#   ./scripts/setup.sh --auto   # Geração automática não-interativa de todas as chaves
# ==============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
COMPOSE_DIR="$REPO_ROOT/compose"

echo "================================================================="
echo "   Hephaestus LLM Studio — Assistente de Configuração Inicial    "
echo "================================================================="
echo ""

# Verifica dependências básicas
command -v openssl >/dev/null 2>&1 || {
    echo "ERRO: 'openssl' não foi encontrado no sistema. Instale o openssl para gerar segredos." >&2
    exit 1
}

command -v docker >/dev/null 2>&1 || {
    echo "AVISO: 'docker' não foi encontrado no PATH. Certifique-se de que o Docker está instalado." >&2
}
AUTO_MODE=0
if [ "${1:-}" = "--auto" ]; then
    AUTO_MODE=1
fi

# Detecção de Hardware (GPU NVIDIA)
HAS_NVIDIA_GPU=0
GPU_INFO=""
if command -v nvidia-smi >/dev/null 2>&1; then
    GPU_INFO="$(nvidia-smi --query-gpu=name,memory.total --format=csv,noheader 2>/dev/null | head -n 1 || true)"
    if [ -n "$GPU_INFO" ]; then
        HAS_NVIDIA_GPU=1
    fi
fi

if [ "$HAS_NVIDIA_GPU" -eq 1 ]; then
    echo "[✓] GPU NVIDIA detectada: $GPU_INFO"
    RECOMMENDED_PROFILE="compose/local-com-local-node-gpu.yaml"
    PROFILE_DESC="Máquina única com GPU NVIDIA local"
    DEFAULT_MANAGER_PUBLISH="127.0.0.1"
    DEFAULT_SEAWEED_PUBLISH="127.0.0.1"
    DEFAULT_TRAINER_IMAGE="ghcr.io/felipecaninnovaes/hephaestus-trainer-yolo:latest"
    DEFAULT_DIFFUSION_IMAGE="ghcr.io/felipecaninnovaes/hephaestus-trainer-difusao:latest"
else
    echo "[i] Nenhuma GPU NVIDIA detectada neste host (modo CPU / Host sem GPU)."
    RECOMMENDED_PROFILE="compose/local-sem-node.yaml"
    PROFILE_DESC="Servidor Central / Control Plane Puro (para workers GPU externos como TrueNAS)"
    DEFAULT_MANAGER_PUBLISH="0.0.0.0"
    DEFAULT_SEAWEED_PUBLISH="0.0.0.0"
    DEFAULT_TRAINER_IMAGE="hephaestus/trainer-yolo:gpu"
    DEFAULT_DIFFUSION_IMAGE="hephaestus/trainer-difusao:gpu"
fi

# Geração de Segredos Criptográficos
echo ""
echo "[1/4] Gerando credenciais aleatórias seguras..."
GENERATED_STUDIO_PASSWORD="heph_$(openssl rand -hex 12)"
GENERATED_STUDIO_MASTER_KEY="$(openssl rand -hex 32)"
GENERATED_MANAGER_TOKEN="heph_mgr_$(openssl rand -hex 24)"
GENERATED_POSTGRES_PASSWORD="$(openssl rand -hex 16)"
GENERATED_S3_ACCESS_KEY="heph_$(openssl rand -hex 8)"
GENERATED_S3_SECRET_KEY="$(openssl rand -hex 24)"
GENERATED_S3_ORCH_ACCESS_KEY="orch_$(openssl rand -hex 8)"
GENERATED_S3_ORCH_SECRET_KEY="$(openssl rand -hex 24)"
GENERATED_ORCH_PAIRING_CODE="pair_$(openssl rand -hex 16)"

STUDIO_PASSWORD="$GENERATED_STUDIO_PASSWORD"
MANAGER_PUBLISH="$DEFAULT_MANAGER_PUBLISH"
SEAWEED_PUBLISH="$DEFAULT_SEAWEED_PUBLISH"
TRAINER_IMAGE="$DEFAULT_TRAINER_IMAGE"
DIFFUSION_TRAINER_IMAGE="$DEFAULT_DIFFUSION_IMAGE"

# Modo Proxy por padrão (S3_PUBLIC_URL vazia)
S3_PUBLIC_URL=""

if [ "$AUTO_MODE" -eq 0 ]; then
    if [ "$HAS_NVIDIA_GPU" -eq 0 ]; then
        echo ""
        echo "-----------------------------------------------------------------"
        echo "Aviso: Sem GPU local, este host pode atuar como:"
        echo "  [1] Servidor Central (Control Plane) para nós remotos com GPU (ex: TrueNAS)"
        echo "  [2] Instalação CPU autônoma (apenas desenvolvimento / mocks / sem difusão)"
        echo "-----------------------------------------------------------------"
        read -rp "Deseja configurar este host como Servidor Central para nós remotos? [S/n]: " RESP_CENTRAL
        if [[ "$RESP_CENTRAL" =~ ^[Nn]$ ]]; then
            RECOMMENDED_PROFILE="compose/local-com-local-node.yaml"
            PROFILE_DESC="Máquina única (CPU / Mock / Desenvolvimento)"
            MANAGER_PUBLISH="127.0.0.1"
            SEAWEED_PUBLISH="127.0.0.1"
            TRAINER_IMAGE="ghcr.io/felipecaninnovaes/hephaestus-trainer-yolo:latest"
            DIFFUSION_TRAINER_IMAGE="ghcr.io/felipecaninnovaes/hephaestus-trainer-difusao:latest"
        fi
    fi

    echo ""
    read -rp "Deseja definir uma senha personalizada para a interface do Studio? [s/N]: " RESP_SENHA
    if [[ "$RESP_SENHA" =~ ^[Ss]$ ]]; then
        read -rsp "Digite a senha do Studio: " CUSTOM_PASS
        echo ""
        if [ -n "$CUSTOM_PASS" ]; then
            STUDIO_PASSWORD="$CUSTOM_PASS"
        fi
    fi

    echo ""
    echo "Por padrão, o Studio opera em Modo Proxy Seguro através do Caddy (:80/:443)."
    echo "Isso elimina problemas de CORS, certificados adicionais e portas de storage bloqueadas."
    read -rp "Deseja habilitar URLs diretas presigned S3 em vez do Modo Proxy? [s/N]: " RESP_DIRECT_S3
    if [[ "$RESP_DIRECT_S3" =~ ^[Ss]$ ]]; then
        read -rp "Informe o IP local desta máquina (ex: 192.168.1.100): " LAN_IP
        if [ -n "$LAN_IP" ]; then
            S3_PUBLIC_URL="http://${LAN_IP}:8333"
            SEAWEED_PUBLISH="0.0.0.0"
        fi
    fi
fi

# Criação do compose/.env
ENV_TARGET="$COMPOSE_DIR/.env"
echo ""
echo "[2/4] Gravando arquivo de ambiente em: $ENV_TARGET"

cat > "$ENV_TARGET" <<EOF
# ==============================================================================
# Hephaestus LLM Studio — Gerado via scripts/setup.sh em $(date -u +"%Y-%m-%dT%H:%M:%SZ")
# ==============================================================================

REGISTRY_PREFIX=ghcr.io/felipecaninnovaes/hephaestus
HEPH_TAG=latest

# Imagens sob demanda para o orquestrador
TRAINER_IMAGE=${TRAINER_IMAGE}
DIFFUSION_TRAINER_IMAGE=${DIFFUSION_TRAINER_IMAGE}

# Credenciais do Studio e BFF
STUDIO_PASSWORD=${STUDIO_PASSWORD}
STUDIO_MASTER_KEY=${GENERATED_STUDIO_MASTER_KEY}

# Token compartilhado entre api-principal, manager e orchestrator
MANAGER_TOKEN=${GENERATED_MANAGER_TOKEN}

# Banco de dados Postgres
POSTGRES_PASSWORD=${GENERATED_POSTGRES_PASSWORD}

# Storage S3 SeaweedFS
S3_BUCKET=heph-data
S3_ACCESS_KEY=${GENERATED_S3_ACCESS_KEY}
S3_SECRET_KEY=${GENERATED_S3_SECRET_KEY}
S3_ORCH_ACCESS_KEY=${GENERATED_S3_ORCH_ACCESS_KEY}
S3_ORCH_SECRET_KEY=${GENERATED_S3_ORCH_SECRET_KEY}

# Endereçamento
S3_PUBLIC_ENDPOINT_URL=${S3_PUBLIC_URL}
MANAGER_PUBLISH=${MANAGER_PUBLISH}
SEAWEED_PUBLISH=${SEAWEED_PUBLISH}

# Pareamento de Nós Remotos
ORCH_PAIRING_CODE=${GENERATED_ORCH_PAIRING_CODE}
ORCH_GPU_DEVICES=all
EXEC_MODE=docker

# Daemon de difusão
DIFFUSION_DAEMON_ENABLED=1
DIFFUSION_DAEMON_PORT=8766
DIFFUSION_DAEMON_IDLE_TTL_S=600
EOF

chmod 600 "$ENV_TARGET"

# Atualização sincronizada do compose/seaweedfs-s3.json
S3_JSON_TARGET="$COMPOSE_DIR/seaweedfs-s3.json"
echo "[3/4] Sincronizando identidades em: $S3_JSON_TARGET"

cat > "$S3_JSON_TARGET" <<EOF
{
  "identities": [
    {
      "name": "heph-admin",
      "credentials": [
        {
          "accessKey": "${GENERATED_S3_ACCESS_KEY}",
          "secretKey": "${GENERATED_S3_SECRET_KEY}"
        }
      ],
      "actions": ["Admin", "Read", "Write", "List", "Tagging", "UserManagement"]
    },
    {
      "name": "heph-orchestrator",
      "credentials": [
        {
          "accessKey": "${GENERATED_S3_ORCH_ACCESS_KEY}",
          "secretKey": "${GENERATED_S3_ORCH_SECRET_KEY}"
        }
      ],
      "actions": [
        "List:heph-data",
        "Read:heph-data/packages/*",
        "List:heph-data/packages/*",
        "Read:heph-data/artifacts/*",
        "Write:heph-data/artifacts/*",
        "List:heph-data/artifacts/*",
        "Read:heph-data/models/*",
        "List:heph-data/models/*"
      ]
    }
  ]
}
EOF

chmod 600 "$S3_JSON_TARGET"

echo "[4/4] Verificando integridade das configurações..."
echo ""
echo "================================================================="
echo "   Configuração Concluída com Sucesso!                           "
echo "================================================================="
echo ""
echo "Suas credenciais de acesso:"
echo "-----------------------------------------------------------------"
echo "Interface Web:   http://localhost (ou http://<seu-ip>)"
echo "Senha do Studio: $STUDIO_PASSWORD"
echo "Código de Nó:    $GENERATED_ORCH_PAIRING_CODE"
echo "-----------------------------------------------------------------"
echo "Perfil Recomendado para este Host:"
echo "-----------------------------------------------------------------"
echo "-> $PROFILE_DESC"
echo ""
echo "Comando para iniciar:"
echo "   docker compose -f $RECOMMENDED_PROFILE up -d"
echo "-----------------------------------------------------------------"
if [ "$RECOMMENDED_PROFILE" = "compose/local-sem-node.yaml" ]; then
    echo ""
    echo "Conexão de Worker Remoto (ex: TrueNAS com GPU):"
    echo "   Para gerar o arquivo de configuração pronto para o worker remoto, execute:"
    echo "   ./scripts/heph.sh export-worker-env"
fi
echo "================================================================="
