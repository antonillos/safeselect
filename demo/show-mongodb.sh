#!/usr/bin/env bash
set -euo pipefail

python3 demo/mcp_call.py mongodb find_documents \
  '{"database":"safeselect_demo","collection":"products","filter":{"category":"security","available":true},"projection":{"_id":0,"sku":1,"name":1,"price":1},"sort":{"price":1},"limit":3}'
