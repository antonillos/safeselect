#!/usr/bin/env bash
# install.sh — build and install safeselect locally
set -euo pipefail

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PREFIX="${PREFIX:-$HOME/.local}"
BIN_DIR="${BIN_DIR:-${PREFIX}/bin}"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --debug) RELEASE="" ;;
    --release|--prod) RELEASE="--release" ;;
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

RELEASE="${RELEASE:---release}"

cd "${SCRIPT_DIR}"

printf 'Building Java sidecar...\n'
mvn -f sidecar/pom.xml package -DskipTests -q
cp sidecar/target/safeselect-sidecar-*.jar sidecar/target/safeselect-sidecar.jar

printf 'Building Rust binary (%s)...\n' "${RELEASE}"
cargo build ${RELEASE} -q

TARGET_DIR="${SCRIPT_DIR}/target/${RELEASE#--}"
printf 'Installing to %s...\n' "${BIN_DIR}"
mkdir -p "${BIN_DIR}"
cp "${TARGET_DIR}/safeselect" "${BIN_DIR}/safeselect"
chmod +x "${BIN_DIR}/safeselect"

printf '\n✓ safeselect installed at %s/safeselect\n' "${BIN_DIR}"
printf '  Make sure %s is in your PATH\n' "${BIN_DIR}"
printf '  Run: safeselect --help\n'
