#!/usr/bin/env bash
set -euo pipefail

printf '%s\n\n' 'USER  > Show me the three most recent paid customer orders.'
printf '%s\n' 'AGENT > I will inspect the approved schema first, then run a bounded read.'
printf '%s\n' 'MCP    > SafeSelect.list_tables'
python3 demo/mcp_call.py postgres list_tables '{}'

printf '\n%s\n' 'AGENT > I found demo_customers and demo_orders. Running a targeted SELECT.'
printf '%s\n' 'MCP    > SafeSelect.select (read-only)'
python3 demo/mcp_call.py postgres select \
  '{"sql":"SELECT c.display_name, o.status, o.subtotal FROM demo_customers c JOIN demo_orders o USING (customer_id) WHERE o.status = '\''paid'\'' ORDER BY o.placed_at DESC LIMIT 3"}'
