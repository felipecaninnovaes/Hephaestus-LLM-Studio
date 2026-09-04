#!/usr/bin/env bash
# Smoke e2e contra o compose local: /health dos 3 serviços.
set -euo pipefail
for svc in 8080 8081 8082; do
  curl -fsS "http://localhost:${svc}/health" && echo " :${svc} ok"
done
