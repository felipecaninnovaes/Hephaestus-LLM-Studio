#!/usr/bin/env bash
# ==============================================================================
# scripts/heph.sh — CLI Operacional Unificada do Hephaestus LLM Studio
# ==============================================================================
# Uso:
#   ./scripts/heph.sh up [--dev|--full|--gpu]
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
    echo "  up [--dev|--full|--gpu]    Inicia os serviços do Studio"
    echo "  down                       Encerra os serviços com segurança"
    echo "  build [--host|--gpu|--all] Compila as imagens Docker"
    echo "  test [--unit|--db|--smoke] Executa testes automatizados"
    echo "  backup                     Cria snapshot seguro do Postgres"
    echo "  doctor                     Executa diagnóstico pré-voo de ambiente"
    echo "  status                     Exibe estado atual dos containers"
    echo ""
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
            if [ -f "$ROOT_DIR/infra/compose.gpu.yaml" ]; then
                echo "Iniciando com perfil GPU..."
                docker compose -f "$COMPOSE_FILE" -f "$ROOT_DIR/infra/compose.gpu.yaml" up -d
            else
                echo "compose.gpu.yaml não encontrado." >&2
                exit 1
            fi
            ;;
        *)
            echo "Opção inválida para 'up': $mode (use --dev, --full ou --gpu)" >&2
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
    help|--help|-h) cmd_help ;;
    *)
        echo "Comando desconhecido: $SUBCMD" >&2
        cmd_help
        exit 1
        ;;
esac
