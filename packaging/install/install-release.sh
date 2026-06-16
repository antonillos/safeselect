#!/usr/bin/env sh
# install-release.sh — standalone POSIX-sh installer for SafeSelect
# Usage: curl -fsSL https://raw.githubusercontent.com/antonillos/safeselect/main/packaging/install/install-release.sh | sh
set -eu

REPO="antonillos/safeselect"
PREFIX="${PREFIX:-${HOME}/.local}"

detect_os_arch() {
    OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
    ARCH="$(uname -m)"
    case "${OS}" in
        darwin) TARGET="apple-darwin" ;;
        linux)  TARGET="unknown-linux-gnu" ;;
        *)      echo "Unsupported OS: ${OS}"; exit 1 ;;
    esac
    case "${ARCH}" in
        aarch64|arm64) TARGET="aarch64-${TARGET}" ;;
        x86_64|amd64)  TARGET="x86_64-${TARGET}" ;;
        *)             echo "Unsupported arch: ${ARCH}"; exit 1 ;;
    esac
}

resolve_latest_version() {
    if [ -n "${SAFESELECT_VERSION:-}" ]; then
        VERSION="${SAFESELECT_VERSION}"
    else
        VERSION=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
            | grep '"tag_name":' \
            | sed 's/.*"v\(.*\)".*/\1/')
    fi
}

download_and_install() {
    URL="https://github.com/${REPO}/releases/download/v${VERSION}/safeselect-${TARGET}.tar.gz"
    TMPDIR=$(mktemp -d)
    cd "${TMPDIR}"

    echo "Downloading SafeSelect v${VERSION} for ${TARGET}..."
    curl -fsSL "${URL}" -o safeselect.tar.gz

    echo "Extracting..."
    tar xzf safeselect.tar.gz

    echo "Installing to ${PREFIX}/bin..."
    mkdir -p "${PREFIX}/bin"
    cp safeselect "${PREFIX}/bin/safeselect"
    chmod +x "${PREFIX}/bin/safeselect"

    rm -rf "${TMPDIR}"

    echo "Installed at ${PREFIX}/bin/safeselect"
    echo "Make sure ${PREFIX}/bin is in your PATH"
}

main() {
    detect_os_arch
    resolve_latest_version
    download_and_install
}

main
