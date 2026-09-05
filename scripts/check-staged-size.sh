#!/usr/bin/env bash
# Aviso (não bloqueio) quando o staged passa do teto de fatia da casa (~400
# linhas de código). Merge commits e docs pesados são exceções conhecidas —
# por isso exit 0 sempre; o bloqueio duro é critério do coordenador.
set -euo pipefail
if ! git rev-parse --verify HEAD >/dev/null 2>&1; then exit 0; fi
# merge/revert em andamento: hook de merge não deve reclamar de tamanho alheio
[ -f "$(git rev-parse --git-dir)/MERGE_HEAD" ] && exit 0

num=$(git diff --cached --numstat --no-color HEAD 2>/dev/null || true)
[ -z "$num" ] && exit 0

lines=$(printf '%s\n' "$num" \
  | grep -vE '(^|[[:space:]])([^[:space:]]*/)?(target/|node_modules/|package-lock\.json|Cargo\.lock|graft/)' \
  | awk '{a+=$1; d+=$2} END {print a+d}')
lines=${lines:-0}

if [ "${lines}" -gt 400 ]; then
  echo "LEFTHOOK (aviso): staged com ~${lines} linhas alteradas > teto de fatia (~400)."
  echo "  Se for fatia de código, quebre em commits menores (ver docs/coordenacao.md)."
  echo "  Exceções: docs/ADRs/migrations de schema — o coordenador decide."
fi
exit 0
