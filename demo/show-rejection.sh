#!/usr/bin/env bash
set -euo pipefail

safeselect query --project demo --environment postgres --sql "DELETE FROM demo_orders"
