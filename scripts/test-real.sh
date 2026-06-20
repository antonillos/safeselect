#!/usr/bin/env bash
set -euo pipefail

# Local script to run real PostgreSQL integration tests
# Usage: ./scripts/test-real.sh [suite]
#   suite: security | smoke | reconnect | all (default: all)
#
# Prerequisites:
#   - Docker running with PostgreSQL container
#   - Set SAFESELECT_SECURITY_DOCKER_CONTAINER=<container-name>
#   - Optionally set SAFESELECT_SECURITY_ADMIN_PASSWORD if different from 'testpass'

SUITE="${1:-all}"

# Default Docker container if not set
export SAFESELECT_SECURITY_DOCKER_CONTAINER="${SAFESELECT_SECURITY_DOCKER_CONTAINER:-compose-postgres-1}"
export SAFESELECT_SECURITY_ADMIN_PASSWORD="${SAFESELECT_SECURITY_ADMIN_PASSWORD:-}"
export SAFESELECT_SECURITY_HOST="${SAFESELECT_SECURITY_HOST:-localhost}"
export SAFESELECT_SECURITY_PORT="${SAFESELECT_SECURITY_PORT:-5432}"
export SAFESELECT_SECURITY_ADMIN_USER="${SAFESELECT_SECURITY_ADMIN_USER:-postgres}"

echo "=== SafeSelect Real Integration Tests ==="
echo "Docker container: $SAFESELECT_SECURITY_DOCKER_CONTAINER"
echo "PostgreSQL: $SAFESELECT_SECURITY_HOST:$SAFESELECT_SECURITY_PORT"
echo "Suite: $SUITE"
echo ""

# Check Docker container is running
if ! docker ps --format '{{.Names}}' | grep -q "^${SAFESELECT_SECURITY_DOCKER_CONTAINER}$"; then
    echo "ERROR: Docker container '$SAFESELECT_SECURITY_DOCKER_CONTAINER' is not running"
    echo "Start your PostgreSQL container first"
    exit 1
fi

# Check psql is available in container
if ! docker exec "$SAFESELECT_SECURITY_DOCKER_CONTAINER" psql -U "$SAFESELECT_SECURITY_ADMIN_USER" -c "SELECT 1" >/dev/null 2>&1; then
    echo "ERROR: Cannot connect to PostgreSQL in container"
    echo "Check container is healthy and credentials are correct"
    exit 1
fi

echo "✓ PostgreSQL connection verified"
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
