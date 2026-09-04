#!/usr/bin/env bash
# Smoke e2e contra o compose local: /health dos 3 serviços.
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
compose() { docker compose -f "$ROOT_DIR/infra/compose.yaml" "$@"; }
for svc in 8080 8081 8082; do
  curl -fsS "http://localhost:${svc}/health" && echo " :${svc} ok"
done
