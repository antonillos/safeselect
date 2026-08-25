#!/usr/bin/env bash
set -euo pipefail

printf '%s\n\n' 'USER  > Which security products are available right now?'
printf '%s\n' 'AGENT > I will use the MongoDB MCP tool with a narrow filter and projection.'
printf '%s\n' 'MCP    > SafeSelect.find_documents (read-only)'
python3 demo/mcp_call.py mongodb find_documents \
  '{"database":"safeselect_demo","collection":"products","filter":{"category":"security","available":true},"projection":{"_id":0,"sku":1,"name":1,"price":1},"sort":{"price":1},"limit":3}'
