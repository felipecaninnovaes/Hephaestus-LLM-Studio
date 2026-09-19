#!/usr/bin/env bash
# ==============================================================================
# scripts/heph.sh — CLI Operacional Unificada do Hephaestus LLM Studio
# ==============================================================================
# Uso:
#   ./scripts/heph.sh up [--dev|--full|--gpu|--prod]
#   ./scripts/heph.sh down
#   ./scripts/heph.sh build [--host|--gpu|--all]
#   ./scripts/heph.sh test [--unit|--db|--storage|--smoke]
#   ./scripts/heph.sh backup
#   ./scripts/heph.sh doctor
#   ./scripts/heph.sh status
# ==============================================================================
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
COMPOSE_FILE="$ROOT_DIR/infra/compose.yaml"

compose() {
    docker compose -f "$COMPOSE_FILE" "$@"
}

cmd_help() {
    echo "Hephaestus LLM Studio — CLI Operacional"
    echo ""
    echo "Uso: $0 <comando> [opções]"
    echo ""
    echo "Comandos:"
    echo "  up [--dev|--full|--gpu|--prod] Inicia os serviços do Studio"
    echo "  down                       Encerra os serviços com segurança"
    echo "  build [--host|--gpu|--all] Compila as imagens Docker"
    echo "  test [--unit|--db|--smoke] Executa testes automatizados"
    echo "  backup                     Cria snapshot seguro do Postgres"
    echo "  doctor                     Executa diagnóstico pré-voo de ambiente"
    echo "  status                     Exibe estado atual dos containers"
    echo "  export-worker-env [IP]     Exporta configuração para nó worker GPU (TrueNAS)"

}

cmd_up() {
    local mode="${1:---dev}"
    case "$mode" in
        --dev)
            echo "Iniciando Hephaestus Studio em modo DEV (backend Docker, frontend local na porta 3000)..."
            compose up -d db seaweedfs s3-init embedder principal manager orchestrator-local
            ;;
        --full)
            echo "Iniciando todos os serviços do Compose..."
            compose up -d
            ;;
        --gpu)
            echo "Iniciando nó GPU no TrueNAS..."
            bash "$ROOT_DIR/scripts/start-truenas.sh"
            ;;
        --prod)
            echo "Iniciando Hephaestus Studio em modo PROD (overlay infra/compose.prod.yaml)..."
            docker compose -f "$COMPOSE_FILE" -f "$ROOT_DIR/infra/compose.prod.yaml" up -d
            ;;
        *)
            echo "Opção inválida para 'up': $mode (use --dev, --full, --gpu ou --prod)" >&2
            exit 1
            ;;
    esac
    echo "✓ Serviços iniciados! Studio disponível em http://localhost:3000"
}

cmd_down() {
    echo "Encerrando serviços do Hephaestus..."
    compose down
    echo "✓ Serviços encerrados com segurança."
}

cmd_build() {
    local target="${1:---host}"
    case "$target" in
        --host)
            echo "Compilando serviços de host (principal, manager, orchestrator, web)..."
            compose build principal manager orchestrator-local web
            ;;
        --gpu)
            echo "Compilando engines GPU..."
            docker compose -f "$COMPOSE_FILE" --profile build build trainer-yolo trainer-difusao
            ;;
        --all)
            echo "Compilando tudo..."
            compose --profile build build
            ;;
        *)
            echo "Opção inválida para 'build': $target (use --host, --gpu ou --all)" >&2
            exit 1
            ;;
    esac
    echo "✓ Build finalizado!"
}

cmd_test() {
    local scope="${1:---unit}"
    case "$scope" in
        --unit)
            echo "Executando testes unitários do workspace Rust..."
            (cd "$ROOT_DIR" && cargo test --workspace)
            echo "Executando testes das engines Python..."
            (cd "$ROOT_DIR/engines/trainer-difusao" && uv run pytest -q)
            (cd "$ROOT_DIR/engines/trainer-yolo" && uv run pytest -q)
            ;;
        --db)
            echo "Executando testes de banco de dados (banco efêmero studio_test)..."
            bash "$ROOT_DIR/scripts/test-db.sh"
            ;;
        --storage)
            echo "Executando testes de storage S3..."
            bash "$ROOT_DIR/scripts/test-storage.sh"
            ;;
        --smoke)
            echo "Executando smoke tests E2E..."
            bash "$ROOT_DIR/scripts/e2e-smoke.sh"
            ;;
        *)
            echo "Opção inválida para 'test': $scope (use --unit, --db, --storage ou --smoke)" >&2
            exit 1
            ;;
    esac
}

cmd_backup() {
    bash "$ROOT_DIR/scripts/backup-db.sh"
}

cmd_doctor() {
    bash "$ROOT_DIR/scripts/doctor.sh"
}

cmd_status() {
    compose ps
}
cmd_export_worker_env() {
    local env_file=""
    if [ -f "$ROOT_DIR/compose/.env" ]; then
        env_file="$ROOT_DIR/compose/.env"
    elif [ -f "$ROOT_DIR/infra/.env" ]; then
        env_file="$ROOT_DIR/infra/.env"
    else
        echo "ERRO: Nenhum arquivo .env encontrado em compose/.env ou infra/.env." >&2
        echo "Execute primeiro ./scripts/setup.sh para gerar as credenciais." >&2
        exit 1
    fi

    local mgr_token="$(grep -E '^MANAGER_TOKEN=' "$env_file" | head -n 1 | cut -d'=' -f2-)"
    local s3_key="$(grep -E '^S3_ORCH_ACCESS_KEY=' "$env_file" | head -n 1 | cut -d'=' -f2-)"
    local s3_secret="$(grep -E '^S3_ORCH_SECRET_KEY=' "$env_file" | head -n 1 | cut -d'=' -f2-)"
    local s3_bucket="$(grep -E '^S3_BUCKET=' "$env_file" | head -n 1 | cut -d'=' -f2-)"
    local orch_pair="$(grep -E '^ORCH_PAIRING_CODE=' "$env_file" | head -n 1 | cut -d'=' -f2-)"

    s3_bucket="${s3_bucket:-heph-data}"

    local control_plane_ip="${1:-}"
    if [ -z "$control_plane_ip" ]; then
        if command -v hostname >/dev/null 2>&1; then
            control_plane_ip="$(hostname -I 2>/dev/null | awk '{print $1}' || true)"
        fi
        if [ -z "$control_plane_ip" ]; then
            control_plane_ip="<IP_DO_CONTROL_PLANE>"
        fi
    fi

    local worker_ip="${2:-<IP_DO_WORKER>}"

    echo "# =============================================================================="
    echo "# Configuração para Nó Worker Remoto (TrueNAS / Servidor GPU)"
    echo "# Gerado automaticamente pelo Hephaestus Control Plane em $(date -u +'%Y-%m-%dT%H:%M:%SZ')"
    echo "# =============================================================================="
    echo "# Cole este conteúdo no arquivo infra/env.gpu ou compose/.env do nó worker com GPU."
    echo ""
    echo "CONTROL_PLANE_IP=${control_plane_ip}"
    echo "NODE_IP=${worker_ip}"
    echo "MANAGER_URL=http://${control_plane_ip}:8081"
    echo "MANAGER_TOKEN=${mgr_token}"
    echo "S3_ORCH_ENDPOINT_URL=http://${control_plane_ip}:8333"
    echo "S3_ORCH_BUCKET=${s3_bucket}"
    echo "S3_ORCH_ACCESS_KEY=${s3_key}"
    echo "S3_ORCH_SECRET_KEY=${s3_secret}"
    echo "ORCH_GPU_DEVICES=0"
    echo "ORCH_ADVERTISE_URL=http://${worker_ip}:8082"
    echo "ORCH_PAIRING_CODE=${orch_pair}"
    echo "ENGINE_MOCK=0"
    echo "HF_TOKEN="
    echo "FLUX_MODEL_ID=black-forest-labs/FLUX.2-klein-base-4B"
    echo "FLUX_DISTILLED_MODEL_ID=black-forest-labs/FLUX.2-klein-4B"
    echo "ENGINE_NETWORK=gpu_default"
    echo "DIFFUSION_DAEMON_NETWORK=gpu_default"
    echo ""
    echo "# Para iniciar o worker no nó GPU (TrueNAS):"
    echo "#   docker compose -p gpu -f infra/compose.gpu.yaml --env-file infra/env.gpu up -d"
    echo "# ou"
    echo "#   docker compose -f compose/remote-node.yaml up -d"
    echo "# =============================================================================="
}


# Entrypoint
SUBCMD="${1:-help}"
shift || true

case "$SUBCMD" in
    up)     cmd_up "$@" ;;
    down)   cmd_down "$@" ;;
    build)  cmd_build "$@" ;;
    test)   cmd_test "$@" ;;
    backup) cmd_backup "$@" ;;
    doctor) cmd_doctor "$@" ;;
    status) cmd_status "$@" ;;
    export-worker-env) cmd_export_worker_env "$@" ;;
    help|--help|-h) cmd_help ;;
    *)
        echo "Comando desconhecido: $SUBCMD" >&2
        cmd_help
        exit 1
        ;;
esac
