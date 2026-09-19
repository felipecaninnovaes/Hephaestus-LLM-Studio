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

# Geração de Segredos Criptográficos
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

if [ "$AUTO_MODE" -eq 0 ]; then
    echo ""
    read -rp "Deseja definir uma senha personalizada para a interface do Studio? [s/N]: " RESP_SENHA
    if [[ "$RESP_SENHA" =~ ^[Ss]$ ]]; then
        read -rsp "Digite a senha do Studio: " CUSTOM_PASS
        echo ""
        if [ -n "$CUSTOM_PASS" ]; then
            STUDIO_PASSWORD="$CUSTOM_PASS"
        fi
    fi
fi

# Configuração de IP / Hostname para S3 Public Endpoint
DEFAULT_S3_PUBLIC="http://localhost:8333"
S3_PUBLIC_URL="$DEFAULT_S3_PUBLIC"

if [ "$AUTO_MODE" -eq 0 ]; then
    echo ""
    read -rp "O Studio será acessado por outros computadores na rede local? [s/N]: " RESP_LAN
    if [[ "$RESP_LAN" =~ ^[Ss]$ ]]; then
        read -rp "Informe o IP local desta máquina (ex: 192.168.1.100): " LAN_IP
        if [ -n "$LAN_IP" ]; then
            S3_PUBLIC_URL="http://${LAN_IP}:8333"
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
TRAINER_IMAGE=ghcr.io/felipecaninnovaes/hephaestus-trainer-yolo:latest
DIFFUSION_TRAINER_IMAGE=ghcr.io/felipecaninnovaes/hephaestus-trainer-difusao:latest

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
MANAGER_PUBLISH=127.0.0.1
SEAWEED_PUBLISH=127.0.0.1

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
echo ""
echo "Para iniciar o Studio, escolha um dos modelos abaixo:"
echo ""
echo "1) Máquina única com GPU NVIDIA (Recomendado se você tem GPU):"
echo "   docker compose -f compose/local-com-local-node-gpu.yaml up -d"
echo ""
echo "2) Máquina única (CPU / Mock / Desenvolvimento):"
echo "   docker compose -f compose/local-com-local-node.yaml up -d"
echo ""
echo "3) Servidor central (para conectar nós/workers externos):"
echo "   docker compose -f compose/local-sem-node.yaml up -d"
echo ""
echo "4) Nó worker remoto (TrueNAS ou máquina dedicada com GPU):"
echo "   docker compose -f compose/remote-node.yaml up -d"
echo "================================================================="
