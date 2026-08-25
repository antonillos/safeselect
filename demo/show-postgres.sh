#!/usr/bin/env bash
set -euo pipefail

safeselect query --project demo --environment postgres --sql \
  "SELECT c.display_name, o.status, o.subtotal
   FROM demo_customers c
   JOIN demo_orders o USING (customer_id)
   WHERE o.status = 'paid'
   ORDER BY o.placed_at DESC
   LIMIT 3"
