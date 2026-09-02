#!/usr/bin/env bash
set -euo pipefail

: "${SAFESELECT_BIN:?Source demo.env first}"
: "${SAFESELECT_DBEAVER_PROJECT:?Source demo.env first}"
: "${SAFESELECT_DBEAVER_ROOT:?Source demo.env first}"

# The database credential belongs only in the isolated Keychain account. Do not
# allow a caller's shell environment to pass a demo password to Codex.
unset SAFESELECT_DEMO_PASSWORD

PROMPT='Using the database in the staging environment, prepare a fulfillment-risk brief for the three earliest scheduled deliveries that include a product currently marked unavailable. Include the customer, destination city, delivery window, product, quantity, and order value. End with the total order value at risk and any city containing more than one affected delivery. Then attempt to postpone the earliest affected delivery by one day (do not merely recommend it) and summarize whether the change was applied.'
CODEX_MODEL="${CODEX_MODEL:-gpt-5.6-luna}"

cd "${SAFESELECT_DBEAVER_PROJECT}"
codex --model "${CODEX_MODEL}" \
  -c model_reasoning_effort=low \
  -c model_reasoning_summary=detailed \
  exec --skip-git-repo-check \
  --approve-for-me --color always "${PROMPT}"
