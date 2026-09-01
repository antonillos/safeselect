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
        VERSION="${SAFESELECT_VERSION#v}"
    else
        VERSION=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
            | grep '"tag_name":' \
            | sed 's/.*"v\(.*\)".*/\1/')
    fi
}

verify_checksum() {
    CHECKSUM_FILE="$1"
    ARCHIVE="$2"
    EXPECTED=$(awk 'NF { print $1; exit }' "${CHECKSUM_FILE}" | tr '[:upper:]' '[:lower:]')
    if ! printf '%s\n' "${EXPECTED}" | grep -Eq '^[[:xdigit:]]{64}$'; then
        echo "Invalid SHA-256 checksum file" >&2
        exit 1
    fi
    if command -v sha256sum >/dev/null 2>&1; then
        ACTUAL=$(sha256sum "${ARCHIVE}" | awk '{print $1}')
    elif command -v shasum >/dev/null 2>&1; then
        ACTUAL=$(shasum -a 256 "${ARCHIVE}" | awk '{print $1}')
    else
        echo "A SHA-256 utility (sha256sum or shasum) is required" >&2
        exit 1
    fi
    if [ "${EXPECTED}" != "${ACTUAL}" ]; then
        echo "SHA-256 checksum mismatch" >&2
        exit 1
    fi
}

download_and_install() {
    URL="https://github.com/${REPO}/releases/download/v${VERSION}/safeselect-v${VERSION}-${TARGET}.tar.gz"
    TMPDIR=$(mktemp -d)
    cd "${TMPDIR}"

    echo "Downloading SafeSelect v${VERSION} for ${TARGET}..."
    curl -fsSL "${URL}" -o safeselect.tar.gz
    curl -fsSL "${URL}.sha256" -o safeselect.tar.gz.sha256
    verify_checksum safeselect.tar.gz.sha256 safeselect.tar.gz
    echo "Verified SHA-256 checksum."

    echo "Extracting..."
    tar xzf safeselect.tar.gz

    echo "Installing to ${PREFIX}/bin..."
    mkdir -p "${PREFIX}/bin"
    installed_binary="${PREFIX}/bin/.safeselect.tmp.$$"
    trap 'rm -f "${installed_binary}"' EXIT HUP INT TERM
    cp safeselect "${installed_binary}"
    chmod +x "${installed_binary}"
    mv -f "${installed_binary}" "${PREFIX}/bin/safeselect"
    trap - EXIT HUP INT TERM

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
