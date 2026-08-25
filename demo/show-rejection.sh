#!/usr/bin/env bash
set -euo pipefail

printf '%s\n\n' 'USER  > Delete every demo order that is not paid.'
printf '%s\n' 'AGENT > I will send the requested action only through my SafeSelect MCP boundary.'
printf '%s\n' 'MCP    > SafeSelect.select (DELETE request)'
python3 demo/mcp_call.py postgres select '{"sql":"DELETE FROM demo_orders WHERE status != '\''paid'\''"}'
