#!/usr/bin/env bash
# install.sh — build and install safeselect locally
set -euo pipefail

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PREFIX="${PREFIX:-$HOME/.local}"
BIN_DIR="${BIN_DIR:-${PREFIX}/bin}"

MODE="release"
RUST_FLAGS="--release"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --debug)
      MODE="debug"
      RUST_FLAGS=""
      ;;
    --release|--prod)
      MODE="release"
      RUST_FLAGS="--release"
      ;;
    --help|-h)
      printf 'Usage: ./install.sh [--release|--debug]\n'
      printf '  --release   Build and install release binary (default)\n'
      printf '  --debug     Build and install debug binary\n'
      exit 0
      ;;
    *) printf 'Error: unknown option: %s\n' "$1" >&2; exit 1 ;;
  esac
  shift
done

cd "${SCRIPT_DIR}"

printf 'Building Java sidecar...\n'
mvn -f sidecar/pom.xml package -DskipTests -q
sidecar_jar="$(ls sidecar/target/safeselect-sidecar-*.jar 2>/dev/null | sort -V | tail -1)"
if [[ -n "$sidecar_jar" ]]; then
  cp "$sidecar_jar" sidecar/target/safeselect-sidecar.jar
fi

printf 'Building Rust binary (%s)...\n' "${MODE}"
RUSTFLAGS="-A warnings" cargo build ${RUST_FLAGS} -q

TARGET_DIR="${SCRIPT_DIR}/target/${MODE}"
printf 'Installing to %s...\n' "${BIN_DIR}"
mkdir -p "${BIN_DIR}"
cp "${TARGET_DIR}/safeselect" "${BIN_DIR}/safeselect"
chmod +x "${BIN_DIR}/safeselect"

printf '\n✓ safeselect installed at %s/safeselect (%s)\n' "${BIN_DIR}" "${MODE}"
printf '  Make sure %s is in your PATH\n' "${BIN_DIR}"
printf '  Run: safeselect --help\n'
