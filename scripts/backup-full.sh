#!/usr/bin/env bash
# ==============================================================================
# scripts/backup-full.sh — Backup completo ordenado: S3 (SeaweedFS) + Postgres
# ==============================================================================
# Snapshot com ordenação referencial S3 -> Postgres:
#   Passo 1: cópia dos objetos do S3 (bucket heph-data) via `rclone copy`
#            (fallback via `docker exec` se o rclone estiver ausente).
#   Passo 2: dump consistente do Postgres via
#            `docker compose exec -T db pg_dump -U studio -d studio -Fc`.
#   Passo 3: salva ambos em `$BACKUP_DIR/full_backup_<TIMESTAMP>/`.
#   Passo 4: rotação dos backups mais antigos mantendo `$BACKUP_KEEP`.
#
# Uso:
#   bash scripts/backup-full.sh          # Executa backup agora
#   bash scripts/backup-full.sh --list   # Lista backups existentes
#   bash scripts/backup-full.sh --help   # Ajuda
#
# Configurações via env:
#   BACKUP_DIR      Diretório de destino (default: ./backups)
#   BACKUP_KEEP     Quantidade de backups mantidos na rotação (default: 7)
#   POSTGRES_USER   Usuário do Postgres (default: studio)
#   POSTGRES_DB     Nome do banco (default: studio)
#   S3_BUCKET       Bucket S3 (default: heph-data)
#   S3_ENDPOINT_URL Endpoint S3 (default: http://localhost:8333)
#   S3_ACCESS_KEY   Chave de acesso S3 (default: heph)
#   S3_SECRET_KEY   Chave secreta S3 (default: heph-local-dev)
# ==============================================================================
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BACKUP_DIR="${BACKUP_DIR:-$ROOT_DIR/backups}"
BACKUP_KEEP="${BACKUP_KEEP:-7}"
POSTGRES_USER="${POSTGRES_USER:-studio}"
POSTGRES_DB="${POSTGRES_DB:-studio}"
S3_BUCKET="${S3_BUCKET:-heph-data}"
S3_ENDPOINT_URL="${S3_ENDPOINT_URL:-http://localhost:8333}"
S3_ACCESS_KEY="${S3_ACCESS_KEY:-heph}"
S3_SECRET_KEY="${S3_SECRET_KEY:-heph-local-dev}"
COMPOSE_FILE="$ROOT_DIR/infra/compose.yaml"

usage() {
    cat <<USAGE
Uso: $(basename "$0") [--help | --list]

Backup completo ordenado (S3 -> Postgres) em \$BACKUP_DIR/full_backup_<TIMESTAMP>/.

  (sem args)  Executa o backup agora.
  --list      Lista os backups existentes.
  --help      Mostra esta ajuda.
USAGE
}

list_backups() {
    echo "=== Backups completos em $BACKUP_DIR ==="
    if find "$BACKUP_DIR" -maxdepth 1 -mindepth 1 -type d -name 'full_backup_*' -print 2>/dev/null | grep -q .; then
        find "$BACKUP_DIR" -maxdepth 1 -mindepth 1 -type d -name 'full_backup_*' -print | sort | while IFS= read -r dir; do
            du -sh "$dir"
        done
    else
        echo "Nenhum backup encontrado."
    fi
}

case "${1:-}" in
    --help|-h) usage; exit 0 ;;
    --list) list_backups; exit 0 ;;
    "") ;;
    *) echo "ERRO: opção desconhecida: $1 (use --help)." >&2; exit 1 ;;
esac

mkdir -p "$BACKUP_DIR"

TIMESTAMP="$(date +"%Y%m%d_%H%M%S")"
DEST="$BACKUP_DIR/full_backup_$TIMESTAMP"
S3_DEST="$DEST/s3"
DB_DUMP="$DEST/${POSTGRES_DB}.dump"
mkdir -p "$S3_DEST"

echo "=== Backup completo: $DEST ==="

# Passo 1: objetos S3 antes do dump (ordenação referencial S3 -> Postgres).
echo "--- Passo 1: objetos S3 (bucket $S3_BUCKET) -> $S3_DEST ---"
if command -v rclone >/dev/null 2>&1; then
    echo "Copiando via rclone..."
    export AWS_ACCESS_KEY_ID="$S3_ACCESS_KEY"
    export AWS_SECRET_ACCESS_KEY="$S3_SECRET_KEY"
    rclone copy ":s3,provider=Minio,env_auth=true,region=us-east-1,endpoint=${S3_ENDPOINT_URL}:${S3_BUCKET}" "$S3_DEST" --s3-no-check-bucket --stats-one-line
else
    echo "rclone ausente — fallback via docker exec (snapshot do volume seaweed_data)..."
    if docker compose -f "$COMPOSE_FILE" ps -q seaweedfs 2>/dev/null | grep -q .; then
        docker compose -f "$COMPOSE_FILE" exec -T seaweedfs tar -czf - -C / data > "$S3_DEST/seaweed_data.tar.gz"
    else
        echo "ERRO: rclone ausente e container 'seaweedfs' não está rodando." >&2
        exit 1
    fi
fi

# Passo 2: dump consistente do Postgres em formato custom (-Fc).
echo "--- Passo 2: dump do Postgres ($POSTGRES_DB) -> $DB_DUMP ---"
if docker compose -f "$COMPOSE_FILE" ps -q db 2>/dev/null | grep -q .; then
    echo "Executando pg_dump via Docker Compose (formato custom -Fc)..."
    docker compose -f "$COMPOSE_FILE" exec -T db \
        pg_dump -U "$POSTGRES_USER" -d "$POSTGRES_DB" -Fc > "$DB_DUMP"
elif command -v pg_dump >/dev/null 2>&1; then
    echo "Container db não detectado. Executando pg_dump nativo..."
    pg_dump -U "$POSTGRES_USER" -d "$POSTGRES_DB" -h "${POSTGRES_HOST:-localhost}" -p "${POSTGRES_PORT:-5432}" -Fc > "$DB_DUMP"
else
    echo "ERRO: Nem container 'db' nem comando 'pg_dump' local estão disponíveis." >&2
    exit 1
fi

if [ -s "$DB_DUMP" ]; then
    SIZE="$(du -h "$DB_DUMP" | cut -f1)"
    echo "✓ Dump concluído: $DB_DUMP ($SIZE)"
else
    echo "ERRO: Dump do Postgres vazio ou ausente." >&2
    exit 1
fi

# Passo 4: rotação dos backups mais antigos (mantém os últimos BACKUP_KEEP).
echo "--- Passo 4: rotação (manter últimos $BACKUP_KEEP) ---"
mapfile -t EXISTING < <(find "$BACKUP_DIR" -maxdepth 1 -mindepth 1 -type d -name 'full_backup_*' -print | sort)
COUNT="${#EXISTING[@]}"
if [ "$COUNT" -gt "$BACKUP_KEEP" ]; then
    EXCESS=$((COUNT - BACKUP_KEEP))
    for ((i = 0; i < EXCESS; i++)); do
        echo "Removendo backup antigo: $(basename "${EXISTING[$i]}")"
        rm -rf "${EXISTING[$i]}"
    done
fi

echo "=== Backup completo finalizado: $DEST ==="
