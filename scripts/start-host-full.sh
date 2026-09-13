#!/usr/bin/env bash
# ==============================================================================
# Atalho para iniciar o ambiente Host no modo FULL (Web NO Docker)
# Sobe todo o ambiente dentro de containers, incluindo o container `web` (Next.js).
# ==============================================================================
set -euo pipefail
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
exec "$DIR/start-host.sh" --with-web "$@"
