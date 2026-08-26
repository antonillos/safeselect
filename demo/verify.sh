#!/usr/bin/env bash
set -euo pipefail

postgres_container="safeselect-demo-postgres-1"
mongodb_container="safeselect-demo-mongodb-1"

printf '%s\n' 'PostgreSQL fixture counts:'
docker exec -i "${postgres_container}" psql -U demo -d safeselect_demo -At <<'SQL'
SELECT 'customers=' || count(*) FROM demo_customers;
SELECT 'products=' || count(*) FROM demo_products;
SELECT 'orders=' || count(*) FROM demo_orders;
SELECT 'order_items=' || count(*) FROM demo_order_items;
SELECT 'events=' || count(*) FROM demo_events;
SQL

printf '%s\n' 'MongoDB fixture counts:'
docker exec "${mongodb_container}" mongosh --quiet \
  'mongodb://demo:demo-password@localhost:27017/safeselect_demo?authSource=admin' \
  --eval "['customers','products','orders','events'].forEach(c => print(c + '=' + db.getSiblingDB('safeselect_demo')[c].countDocuments({})))"

printf '%s\n' 'Fixture verification passed.'
