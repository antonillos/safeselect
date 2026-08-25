#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(CDPATH= cd -- "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
COMPOSE=(docker compose -p safeselect-demo -f "${ROOT_DIR}/docker-compose.yml")

case "${1:-}" in
  start)
    "${COMPOSE[@]}" up -d --wait
    printf '\nDemo databases are ready.\n'
    printf '  PostgreSQL: jdbc:postgresql://127.0.0.1:55432/safeselect_demo\n'
    printf '  MongoDB:    mongodb://demo:demo-password@127.0.0.1:57017/safeselect_demo?authSource=admin\n'
    ;;
  stop)
    "${COMPOSE[@]}" down
    ;;
  reset)
    "${COMPOSE[@]}" down -v
    "${COMPOSE[@]}" up -d --wait
    ;;
  status)
    "${COMPOSE[@]}" ps
    ;;
  logs)
    "${COMPOSE[@]}" logs --tail=80
    ;;
  *)
    printf 'Usage: %s {start|stop|reset|status|logs}\n' "$0" >&2
    exit 2
    ;;
esac
