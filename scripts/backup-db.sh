#!/usr/bin/env bash
# ==============================================================================
# scripts/backup-db.sh — Rotina de Backup Automatizado do Postgres (pgvector)
# ==============================================================================
# Cria snapshots compactados (.sql.gz) do banco de dados `studio`.
# Operação 100% não-destrutiva (somente leitura via pg_dump).
#
# Uso:
#   bash scripts/backup-db.sh              # Executa backup agora
#   bash scripts/backup-db.sh --list       # Lista backups existentes
#
# Configurações via env:
#   BACKUP_DIR      Diretório de destino (default: ./backups)
#   BACKUP_KEEP     Quantidade de backups mantidos na rotação (default: 7)
#   POSTGRES_USER   Usuário do Postgres (default: studio)
#   POSTGRES_DB     Nome do banco (default: studio)
# ==============================================================================
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BACKUP_DIR="${BACKUP_DIR:-$ROOT_DIR/backups}"
BACKUP_KEEP="${BACKUP_KEEP:-7}"
POSTGRES_USER="${POSTGRES_USER:-studio}"
POSTGRES_DB="${POSTGRES_DB:-studio}"

mkdir -p "$BACKUP_DIR"

if [[ "${1:-}" == "--list" ]]; then
    echo "=== Backups disponíveis em $BACKUP_DIR ==="
    ls -lh "$BACKUP_DIR"/*.sql.gz 2>/dev/null || echo "Nenhum backup encontrado."
    exit 0
fi

TIMESTAMP="$(date +"%Y%m%d_%H%M%S")"
BACKUP_FILE="$BACKUP_DIR/${POSTGRES_DB}_backup_${TIMESTAMP}.sql.gz"

echo "=== Iniciando Backup do Postgres [$POSTGRES_DB] ==="
echo "Destino: $BACKUP_FILE"

# Verifica se o container db está de pé
if docker compose -f "$ROOT_DIR/infra/compose.yaml" ps -q db 2>/dev/null | grep -q .; then
    echo "Executando pg_dump via Docker Compose..."
    docker compose -f "$ROOT_DIR/infra/compose.yaml" exec -T db \
        pg_dump -U "$POSTGRES_USER" -d "$POSTGRES_DB" --clean --if-exists | gzip > "$BACKUP_FILE"
elif command -v pg_dump >/dev/null 2>&1; then
    echo "Container db não detectado. Executando pg_dump nativo..."
    pg_dump -U "$POSTGRES_USER" -d "$POSTGRES_DB" -h "${POSTGRES_HOST:-localhost}" -p "${POSTGRES_PORT:-5432}" --clean --if-exists | gzip > "$BACKUP_FILE"
else
    echo "ERRO: Nem container 'db' nem comando 'pg_dump' local estão disponíveis." >&2
    exit 1
fi

# Validação de integridade do arquivo compactado
if gzip -t "$BACKUP_FILE" 2>/dev/null; then
    SIZE="$(du -h "$BACKUP_FILE" | cut -f1)"
    echo "✓ Backup concluído com sucesso: $BACKUP_FILE ($SIZE)"
else
    echo "ERRO: Arquivo de backup corrompido ou vazio." >&2
    rm -f "$BACKUP_FILE"
    exit 1
fi

# Rotação de backups antigos (mantém os últimos BACKUP_KEEP)
echo "Aplicando rotação de backups (manter últimos $BACKUP_KEEP)..."
COUNT=$(ls -1t "$BACKUP_DIR"/${POSTGRES_DB}_backup_*.sql.gz 2>/dev/null | wc -l)
if [ "$COUNT" -gt "$BACKUP_KEEP" ]; then
    EXCESS=$((COUNT - BACKUP_KEEP))
    ls -1t "$BACKUP_DIR"/${POSTGRES_DB}_backup_*.sql.gz | tail -n "$EXCESS" | while read -r old_file; do
        echo "Removendo backup antigo: $(basename "$old_file")"
        rm -f "$old_file"
    done
fi

echo "=== Rotina de backup finalizada ==="
