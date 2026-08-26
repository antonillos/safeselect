#!/usr/bin/env bash
set -euo pipefail

CODEX_WORKSPACE="${CODEX_WORKSPACE:-$(pwd)}"
LOG_FILE="$(mktemp "${TMPDIR:-/tmp}/safeselect-codex.XXXXXX")"
trap 'rm -f "${LOG_FILE}"' EXIT

set +e
codex exec --ephemeral --enable fast_mode \
  --cd "${CODEX_WORKSPACE}" \
  --approve-for-me \
  --skip-git-repo-check \
  'Which three customers have the most recent paid orders? Give me their names and order totals.' \
  2>&1 | tee "${LOG_FILE}" | sed -E '/ WARN |guardian trunk rollout snapshot|Session persistence is disabled|guardian review fork/d'
PIPE_STATUS=("${PIPESTATUS[@]}")
CODEX_STATUS="${PIPE_STATUS[0]}"
set -e

printf '\n--- SafeSelect MCP calls ---\n'
grep '^mcp: safeselect' "${LOG_FILE}" || true

printf '\n--- Agent final answer ---\n'
tail -n 20 "${LOG_FILE}" | sed -E '/ WARN |guardian trunk rollout snapshot|Session persistence is disabled|guardian review fork/d'

exit "${CODEX_STATUS}"
