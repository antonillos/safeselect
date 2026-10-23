#!/usr/bin/env bash
# install.sh — build and install safeselect locally
set -euo pipefail

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PREFIX="${PREFIX:-$HOME/.local}"
BIN_DIR="${BIN_DIR:-${PREFIX}/bin}"

MODE="release"
RUST_FLAGS="--release"
INSTALL_MAKEVN=false
UPDATE=false
EXPECTED_BRANCH=""

if [[ -t 1 && -z "${NO_COLOR:-}" && "${TERM:-}" != "dumb" ]]; then
  SUCCESS_MARKER=$'\033[32m✓\033[0m'
else
  SUCCESS_MARKER='✓'
fi

while [[ $# -gt 0 ]]; do
  case "$1" in
    --branch)
      if [[ $# -lt 2 || -z "$2" ]]; then
        printf 'Error: --branch requires a branch name.\n' >&2
        exit 1
      fi
      EXPECTED_BRANCH="$2"
      shift
      ;;
    --update)
      UPDATE=true
      ;;
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
      printf 'Usage: ./install.sh [--release|--debug] [--install-makevn] [--update] [--branch <name>]\n'
      printf '  --release   Build and install release binary (default)\n'
      printf '  --branch    Require this installed source branch when updating\n'
      printf '  --update    Fast-forward the clean local checkout before installing\n'
      printf '  --debug     Build and install debug binary\n'
      printf '  --install-makevn  Install missing makevn with Homebrew or asdf\n'
      exit 0
      ;;
    *) printf 'Error: unknown option: %s\n' "$1" >&2; exit 1 ;;
  esac
  shift
done

cd "${SCRIPT_DIR}"

if [[ "${UPDATE}" == true ]]; then
  if [[ "$(git rev-parse --show-toplevel 2>/dev/null)" != "${SCRIPT_DIR}" ]]; then
    printf 'Error: --update requires the SafeSelect Git checkout.\n' >&2
    exit 1
  fi
  if [[ -n "$(git status --porcelain)" ]]; then
    printf 'Error: commit or stash local changes before updating.\n' >&2
    exit 1
  fi
  if ! git symbolic-ref -q HEAD >/dev/null || ! git rev-parse --verify '@{upstream}' >/dev/null 2>&1; then
    printf 'Error: --update requires a branch with an upstream.\n' >&2
    exit 1
  fi
  if [[ -n "${EXPECTED_BRANCH}" && "$(git symbolic-ref --short -q HEAD)" != "${EXPECTED_BRANCH}" ]]; then
    printf 'Error: checkout does not match the installed source branch; switch branches manually before updating.\n' >&2
    exit 1
  fi
  git pull --ff-only
  # Run the updated installer, preserving the selected installation settings.
  update_args=("--${MODE}")
  if [[ "${INSTALL_MAKEVN}" == true ]]; then
    update_args+=(--install-makevn)
  fi
  exec env BIN_DIR="${BIN_DIR}" bash "${SCRIPT_DIR}/install.sh" "${update_args[@]}"
fi

ensure_makevn() {
  if command -v makevn >/dev/null 2>&1 && makevn --version >/dev/null 2>&1; then
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
    makevn_prefix="$(brew --prefix antonillos/tap/makevn)"
    MAKEVN_COMMAND=("${makevn_prefix}/bin/makevn")
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
    asdf reshim makevn "${makevn_version}"
    MAKEVN_COMMAND=(env "ASDF_MAKEVN_VERSION=${makevn_version}" asdf exec makevn)
  else
    printf 'Error: makevn is missing and neither Homebrew nor asdf is available.\n' >&2
    printf 'Install makevn manually, then rerun ./install.sh.\n' >&2
    return 1
  fi

  if ! "${MAKEVN_COMMAND[@]}" --version >/dev/null 2>&1; then
    printf 'Error: makevn installation completed but the selected executable cannot run.\n' >&2
    return 1
  fi
}

MAKEVN_COMMAND=(makevn)
ensure_makevn

printf 'Building Java sidecar...\n'
"${MAKEVN_COMMAND[@]}" doctor --compact
"${MAKEVN_COMMAND[@]}" init --force
"${MAKEVN_COMMAND[@]}" test package
sidecar_jar="$(ls sidecar/target/safeselect-sidecar-*.jar 2>/dev/null | sort -V | tail -1)"
if [[ -n "$sidecar_jar" ]]; then
  cp "$sidecar_jar" sidecar/target/safeselect-sidecar.jar
fi

base_version="$(sed -nE 's/^version = "([^"]+)"/\1/p' Cargo.toml | head -1)"
build_stamp="$(date +"%Y.%m.%d.%H.%M")"
build_version="${base_version} (${build_stamp})"

source_branch="$(git symbolic-ref --short -q HEAD 2>/dev/null || true)"

printf 'Building Rust binary (%s)...\n' "${MODE}"
SAFESELECT_SOURCE_BRANCH="${source_branch}" SAFESELECT_SOURCE_DIR="${SCRIPT_DIR}" SAFESELECT_SOURCE_MODE="${MODE}" SAFESELECT_BUILD_VERSION="${build_version}" RUSTFLAGS="-A warnings" cargo build ${RUST_FLAGS} -q

TARGET_DIR="${SCRIPT_DIR}/target/${MODE}"
printf 'Installing to %s...\n' "${BIN_DIR}"
mkdir -p "${BIN_DIR}"
installed_binary="${BIN_DIR}/.safeselect.tmp.$$"
trap 'rm -f "${installed_binary}"' EXIT
cp "${TARGET_DIR}/safeselect" "${installed_binary}"
chmod +x "${installed_binary}"
mv -f "${installed_binary}" "${BIN_DIR}/safeselect"
trap - EXIT

printf '\n%s safeselect installed at %s/safeselect (%s)\n' "${SUCCESS_MARKER}" "${BIN_DIR}" "${MODE}"
printf '  Make sure %s is in your PATH\n' "${BIN_DIR}"
printf '  Run: safeselect --help\n'
