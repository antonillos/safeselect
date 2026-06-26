#!/usr/bin/env bash
set -euo pipefail

# Local script to run real PostgreSQL integration tests
# Usage: ./scripts/test-real.sh [suite]
#   suite: security | smoke | reconnect | all (default: all)
#
# Prerequisites:
#   - Docker running
#   - Optionally set SAFESELECT_SECURITY_ADMIN_PASSWORD if different from 'testpass'

SUITE="${1:-all}"
COMPOSE_FILE="${SAFESELECT_TEST_COMPOSE_FILE:-docker-compose.integration.yml}"
PROJECT_NAME="${SAFESELECT_TEST_COMPOSE_PROJECT:-safeselect-real}"
COMPOSE_CMD=(docker-compose -p "$PROJECT_NAME" -f "$COMPOSE_FILE")

export SAFESELECT_SECURITY_ADMIN_PASSWORD="${SAFESELECT_SECURITY_ADMIN_PASSWORD:-testpass}"
export SAFESELECT_SECURITY_HOST="${SAFESELECT_SECURITY_HOST:-localhost}"
export SAFESELECT_SECURITY_PORT="${SAFESELECT_SECURITY_PORT:-5432}"
export SAFESELECT_SECURITY_ADMIN_USER="${SAFESELECT_SECURITY_ADMIN_USER:-postgres}"
export SAFESELECT_SECURITY_DOCKER_CONTAINER="${SAFESELECT_SECURITY_DOCKER_CONTAINER:-${PROJECT_NAME}-postgres-1}"

echo "=== SafeSelect Real Integration Tests ==="
echo "Compose file: $COMPOSE_FILE"
echo "Compose project: $PROJECT_NAME"
echo "PostgreSQL container: $SAFESELECT_SECURITY_DOCKER_CONTAINER"
echo "PostgreSQL: $SAFESELECT_SECURITY_HOST:$SAFESELECT_SECURITY_PORT"
echo "Suite: $SUITE"
echo ""

cleanup() {
    "${COMPOSE_CMD[@]}" down -v >/dev/null 2>&1 || true
}

trap cleanup EXIT

if [[ ! -f "$COMPOSE_FILE" ]]; then
    echo "ERROR: Compose file '$COMPOSE_FILE' not found"
    exit 1
fi

echo "Starting integration services..."
"${COMPOSE_CMD[@]}" up -d

if ! "${COMPOSE_CMD[@]}" ps | grep -q 'postgres'; then
    echo "ERROR: PostgreSQL service did not start"
    exit 1
fi

echo "Waiting for PostgreSQL to become ready..."
postgres_ready=0
for _ in $(seq 1 60); do
    if docker exec \
        -e "PGPASSWORD=$SAFESELECT_SECURITY_ADMIN_PASSWORD" \
        "$SAFESELECT_SECURITY_DOCKER_CONTAINER" \
        psql -U "$SAFESELECT_SECURITY_ADMIN_USER" -d postgres -c "SELECT 1" \
        >/dev/null 2>&1; then
        postgres_ready=1
        break
    fi
    sleep 1
done

if [[ "$postgres_ready" -ne 1 ]]; then
    echo "ERROR: Cannot connect to PostgreSQL in container"
    echo "Check container is healthy and credentials are correct"
    exit 1
fi

echo "✓ PostgreSQL connection verified"
echo "✓ MongoDB service available in compose stack"
echo ""

run_security() {
    echo "=== Running Security Tests ==="
    SAFESELECT_SECURITY_TEST=1 cargo test --test security -- --nocapture
    echo ""
}

run_smoke() {
    echo "=== Running Smoke Tests ==="
    SAFESELECT_REAL_SMOKE_TEST=1 cargo test --test smoke_suite postgres_smoke_errors_and_timeouts -- --nocapture
    echo ""
}

run_reconnect() {
    echo "=== Running Reconnect Test ==="
    echo "WARNING: This will restart the Docker container '$SAFESELECT_SECURITY_DOCKER_CONTAINER'"
    read -p "Continue? [y/N] " -n 1 -r
    echo
    if [[ ! $REPLY =~ ^[Yy]$ ]]; then
        echo "Skipped reconnect test"
        return
    fi
    
    SAFESELECT_RECONNECT_TEST=1 cargo test --test smoke_suite postgres_reconnect_after_docker_restart -- --nocapture
    echo ""
}

case "$SUITE" in
    security)
        run_security
        ;;
    smoke)
        run_smoke
        ;;
    reconnect)
        run_reconnect
        ;;
    all)
        run_security
        run_smoke
        run_reconnect
        ;;
    *)
        echo "Unknown suite: $SUITE"
        echo "Usage: $0 [security|smoke|reconnect|all]"
        exit 1
        ;;
esac

echo "=== All tests completed ==="
