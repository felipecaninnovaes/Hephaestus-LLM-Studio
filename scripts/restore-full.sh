#!/usr/bin/env bash
# ==============================================================================
# scripts/restore-full.sh — Restore completo: Postgres + objetos S3
# ==============================================================================
# Restaura um backup gerado por scripts/backup-full.sh. Ordem inversa à do
# backup (Postgres -> S3): primeiro as linhas do banco
# (`pg_restore --clean`), depois os objetos S3 referenciados por elas
# (`rclone copy` de volta, ou fallback via `docker exec`). Assim o banco
# nunca aponta para objetos ainda não restaurados.
#
# PROTEÇÃO: sem `--yes` o script faz apenas dry-run informativo e sai com
# exit 1 — nunca deleta/sobrescreve nada sem confirmação explícita.
#
# Uso:
#   bash scripts/restore-full.sh <backup-dir-ou-arquivo>        # dry-run (exit 1)
#   bash scripts/restore-full.sh --yes <backup-dir-ou-arquivo>  # restaura de verdade
#   bash scripts/restore-full.sh --help
#
# Configurações via env (mesmas do backup-full.sh):
#   POSTGRES_USER / POSTGRES_DB / S3_BUCKET / S3_ENDPOINT_URL
#   S3_ACCESS_KEY / S3_SECRET_KEY
# ==============================================================================
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
POSTGRES_USER="${POSTGRES_USER:-studio}"
POSTGRES_DB="${POSTGRES_DB:-studio}"
S3_BUCKET="${S3_BUCKET:-heph-data}"
S3_ENDPOINT_URL="${S3_ENDPOINT_URL:-http://localhost:8333}"
S3_ACCESS_KEY="${S3_ACCESS_KEY:-heph}"
S3_SECRET_KEY="${S3_SECRET_KEY:-heph-local-dev}"
COMPOSE_FILE="$ROOT_DIR/infra/compose.yaml"

usage() {
    cat <<USAGE
Uso: $(basename "$0") [--yes] <backup-dir-ou-arquivo>

Restaura Postgres + objetos S3 a partir de um backup do backup-full.sh.

  <backup-dir-ou-arquivo>  Diretório full_backup_<TIMESTAMP>/ ou arquivo .dump
                           avulso (neste caso só o Postgres é restaurado).
  --yes                    Confirmação explícita: executa o restore de verdade.
                           Sem ela, faz apenas dry-run informativo (exit 1).
  --help                   Mostra esta ajuda.
USAGE
}

YES=0
SRC=""

while [ $# -gt 0 ]; do
    case "$1" in
        --help|-h) usage; exit 0 ;;
        --yes) YES=1; shift ;;
        --*) echo "ERRO: opção desconhecida: $1 (use --help)." >&2; exit 1 ;;
        *)
            if [ -n "$SRC" ]; then
                echo "ERRO: múltiplas origens informadas (use --help)." >&2
                exit 1
            fi
            SRC="$1"; shift ;;
    esac
done

if [ -z "$SRC" ]; then
    usage >&2
    exit 1
fi

# Resolve dump do Postgres e origem S3 a partir do caminho informado.
DB_DUMP=""
S3_SRC=""
if [ -d "$SRC" ]; then
    mapfile -t DUMPS < <(find "$SRC" -maxdepth 1 -type f -name '*.dump' -print | sort)
    if [ "${#DUMPS[@]}" -eq 0 ]; then
        echo "ERRO: nenhum arquivo *.dump em $SRC." >&2
        exit 1
    fi
    DB_DUMP="${DUMPS[0]}"
    if [ -d "$SRC/s3" ]; then
        S3_SRC="$SRC/s3"
    fi
elif [ -f "$SRC" ]; then
    DB_DUMP="$SRC"
    echo "AVISO: arquivo avulso — somente o Postgres será restaurado (S3 ignorado)."
else
    echo "ERRO: caminho inexistente: $SRC" >&2
    exit 1
fi

# Sem --yes: dry-run informativo, exit 1 (proteção contra deleções acidentais).
if [ "$YES" -ne 1 ]; then
    cat <<EOF
=== DRY-RUN (nada foi alterado) ===
Origem: $SRC
  1. Postgres: pg_restore --clean --if-exists -U $POSTGRES_USER -d $POSTGRES_DB < $DB_DUMP
  2. S3:       $(if [ -n "$S3_SRC" ]; then echo "rclone copy $S3_SRC -> bucket $S3_BUCKET"; else echo "(ignorado — sem objetos S3 no backup)"; fi)

Passe --yes para executar o restore de verdade.
EOF
    exit 1
fi

echo "=== Restore Postgres: $DB_DUMP ==="
if docker compose -f "$COMPOSE_FILE" ps -q db 2>/dev/null | grep -q .; then
    echo "Restaurando via Docker Compose (pg_restore --clean)..."
    docker compose -f "$COMPOSE_FILE" exec -T db \
        pg_restore -U "$POSTGRES_USER" -d "$POSTGRES_DB" --clean --if-exists < "$DB_DUMP"
elif command -v pg_restore >/dev/null 2>&1; then
    echo "Container db não detectado. Restaurando via pg_restore nativo..."
    pg_restore -U "$POSTGRES_USER" -d "$POSTGRES_DB" -h "${POSTGRES_HOST:-localhost}" -p "${POSTGRES_PORT:-5432}" --clean --if-exists "$DB_DUMP"
else
    echo "ERRO: Nem container 'db' nem comando 'pg_restore' local estão disponíveis." >&2
    exit 1
fi
echo "✓ Postgres restaurado."

echo "=== Restore S3 (bucket $S3_BUCKET) ==="
if [ -z "$S3_SRC" ]; then
    echo "Sem objetos S3 no backup — etapa ignorada."
elif [ -f "$S3_SRC/seaweed_data.tar.gz" ]; then
    echo "Snapshot de volume detectado — restaurando via docker exec..."
    if docker compose -f "$COMPOSE_FILE" ps -q seaweedfs 2>/dev/null | grep -q .; then
        docker compose -f "$COMPOSE_FILE" exec -T seaweedfs tar -xzf - -C / < "$S3_SRC/seaweed_data.tar.gz"
    else
        echo "ERRO: container 'seaweedfs' não está rodando." >&2
        exit 1
    fi
elif command -v rclone >/dev/null 2>&1; then
    echo "Copiando objetos de volta via rclone..."
    export AWS_ACCESS_KEY_ID="$S3_ACCESS_KEY"
    export AWS_SECRET_ACCESS_KEY="$S3_SECRET_KEY"
    rclone copy "$S3_SRC" ":s3,provider=Minio,env_auth=true,region=us-east-1,endpoint=${S3_ENDPOINT_URL}:${S3_BUCKET}" --s3-no-check-bucket --stats-one-line
else
    echo "ERRO: rclone ausente e backup S3 não é snapshot de volume — instale o rclone." >&2
    exit 1
fi
echo "✓ Restore S3 concluído."

echo "=== Restore completo finalizado (origem: $SRC) ==="
