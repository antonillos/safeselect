#!/usr/bin/env bash
# install.sh — build and install safeselect locally
set -euo pipefail

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PREFIX="${PREFIX:-$HOME/.local}"
BIN_DIR="${BIN_DIR:-${PREFIX}/bin}"

MODE="release"
RUST_FLAGS="--release"
INSTALL_MAKEVN=false

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
    --install-makevn)
      INSTALL_MAKEVN=true
      ;;
    --help|-h)
      printf 'Usage: ./install.sh [--release|--debug] [--install-makevn]\n'
      printf '  --release   Build and install release binary (default)\n'
      printf '  --debug     Build and install debug binary\n'
      printf '  --install-makevn  Install missing makevn with Homebrew or asdf\n'
      exit 0
      ;;
    *) printf 'Error: unknown option: %s\n' "$1" >&2; exit 1 ;;
  esac
  shift
done

cd "${SCRIPT_DIR}"

ensure_makevn() {
  if command -v makevn >/dev/null 2>&1; then
    return
  fi

  if [[ "${INSTALL_MAKEVN}" != true ]]; then
    printf 'Error: makevn is required to build the Java sidecar.\n' >&2
    printf 'Install it first, or rerun: ./install.sh --install-makevn\n' >&2
    return 1
  fi

  if command -v brew >/dev/null 2>&1; then
    printf 'Installing makevn with Homebrew...\n'
    brew install antonillos/tap/makevn
  elif command -v asdf >/dev/null 2>&1; then
    printf 'Installing makevn with asdf...\n'
    if ! asdf plugin list | grep -Fxq 'makevn'; then
      asdf plugin add makevn https://github.com/antonillos/asdf-makevn.git
    fi
    makevn_version="$(asdf latest makevn | tail -n 1)"
    if [[ -z "${makevn_version}" ]]; then
      printf 'Error: asdf did not report a makevn version.\n' >&2
      return 1
    fi
    if ! asdf list makevn "${makevn_version}" >/dev/null 2>&1; then
      asdf install makevn "${makevn_version}"
    fi
    asdf set -u makevn "${makevn_version}"
    asdf reshim makevn "${makevn_version}"
  else
    printf 'Error: makevn is missing and neither Homebrew nor asdf is available.\n' >&2
    printf 'Install makevn manually, then rerun ./install.sh.\n' >&2
    return 1
  fi

  if ! command -v makevn >/dev/null 2>&1; then
    printf 'Error: makevn installation completed but makevn is not on PATH.\n' >&2
    return 1
  fi
}

ensure_makevn

printf 'Building Java sidecar...\n'
makevn package
sidecar_jar="$(ls sidecar/target/safeselect-sidecar-*.jar 2>/dev/null | sort -V | tail -1)"
if [[ -n "$sidecar_jar" ]]; then
  cp "$sidecar_jar" sidecar/target/safeselect-sidecar.jar
fi

base_version="$(sed -nE 's/^version = "([^"]+)"/\1/p' Cargo.toml | head -1)"
build_stamp="$(date +"%Y.%m.%d.%H.%M")"
build_version="${base_version} (${build_stamp})"

printf 'Building Rust binary (%s)...\n' "${MODE}"
SAFESELECT_BUILD_VERSION="${build_version}" RUSTFLAGS="-A warnings" cargo build ${RUST_FLAGS} -q

TARGET_DIR="${SCRIPT_DIR}/target/${MODE}"
printf 'Installing to %s...\n' "${BIN_DIR}"
mkdir -p "${BIN_DIR}"
installed_binary="${BIN_DIR}/.safeselect.tmp.$$"
trap 'rm -f "${installed_binary}"' EXIT
cp "${TARGET_DIR}/safeselect" "${installed_binary}"
chmod +x "${installed_binary}"
mv -f "${installed_binary}" "${BIN_DIR}/safeselect"
trap - EXIT

printf '\n✓ safeselect installed at %s/safeselect (%s)\n' "${BIN_DIR}" "${MODE}"
printf '  Make sure %s is in your PATH\n' "${BIN_DIR}"
printf '  Run: safeselect --help\n'
