#!/usr/bin/env bash
# ==============================================================================
# Atalho para iniciar o ambiente Host no modo DEV (Web fora do Docker)
# O backend sobe no Docker e o frontend Next.js roda no host (`npm run dev`).
# ==============================================================================
set -euo pipefail
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
exec "$DIR/start-host.sh" --no-web "$@"
