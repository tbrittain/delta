#!/usr/bin/env bash
# delta installer
# Usage: curl -sSf https://raw.githubusercontent.com/tbrittain/delta/main/install.sh | bash
#    or: bash install.sh [--install-dir DIR]
set -euo pipefail

REPO="tbrittain/delta"
BINARY="delta"
DEFAULT_INSTALL_DIR="${HOME}/.local/bin"

# ── Argument parsing ──────────────────────────────────────────────────────────

INSTALL_DIR="${DEFAULT_INSTALL_DIR}"

while [[ $# -gt 0 ]]; do
    case "$1" in
        --install-dir)
            INSTALL_DIR="$2"
            shift 2
            ;;
        --help|-h)
            echo "Usage: install.sh [--install-dir DIR]"
            echo ""
            echo "Options:"
            echo "  --install-dir DIR   Install delta to DIR (default: ~/.local/bin)"
            exit 0
            ;;
        *)
            echo "Unknown option: $1" >&2
            exit 1
            ;;
    esac
done

# ── Platform detection ────────────────────────────────────────────────────────

OS=$(uname -s)
ARCH=$(uname -m)

case "${OS}" in
    Linux)
        case "${ARCH}" in
            x86_64)  TARGET="x86_64-unknown-linux-gnu" ;;
            aarch64|arm64) TARGET="aarch64-unknown-linux-gnu" ;;
            *) echo "Unsupported architecture: ${ARCH}" >&2; exit 1 ;;
        esac
        ARCHIVE_EXT="tar.gz"
        ;;
    Darwin)
        case "${ARCH}" in
            x86_64)  TARGET="x86_64-apple-darwin" ;;
            arm64)   TARGET="aarch64-apple-darwin" ;;
            *) echo "Unsupported architecture: ${ARCH}" >&2; exit 1 ;;
        esac
        ARCHIVE_EXT="tar.gz"
        ;;
    *)
        echo "Unsupported OS: ${OS}" >&2
        echo "Windows users: download the .zip from https://github.com/${REPO}/releases/latest" >&2
        exit 1
        ;;
esac

# ── Fetch latest release ──────────────────────────────────────────────────────

echo "Detecting latest release..."

if command -v curl &>/dev/null; then
    FETCH="curl -sSfL"
elif command -v wget &>/dev/null; then
    FETCH="wget -qO-"
else
    echo "curl or wget is required" >&2
    exit 1
fi

API_URL="https://api.github.com/repos/${REPO}/releases/latest"
VERSION=$(${FETCH} "${API_URL}" | grep '"tag_name"' | sed 's/.*"tag_name": *"\([^"]*\)".*/\1/')

if [[ -z "${VERSION}" ]]; then
    echo "Could not determine latest version. Is the repo public?" >&2
    exit 1
fi

ARCHIVE_NAME="${BINARY}-${VERSION}-${TARGET}.${ARCHIVE_EXT}"
DOWNLOAD_URL="https://github.com/${REPO}/releases/download/${VERSION}/${ARCHIVE_NAME}"

echo "Installing ${BINARY} ${VERSION} for ${TARGET}"
echo "From: ${DOWNLOAD_URL}"

# ── Download and extract ──────────────────────────────────────────────────────

TMP_DIR=$(mktemp -d)
trap 'rm -rf "${TMP_DIR}"' EXIT

echo "Downloading..."
${FETCH} "${DOWNLOAD_URL}" -o "${TMP_DIR}/${ARCHIVE_NAME}"

echo "Extracting..."
tar -xzf "${TMP_DIR}/${ARCHIVE_NAME}" -C "${TMP_DIR}"

# ── Install ───────────────────────────────────────────────────────────────────

mkdir -p "${INSTALL_DIR}"

DEST="${INSTALL_DIR}/${BINARY}"

if [[ -f "${DEST}" ]]; then
    echo "Replacing existing install at ${DEST}"
    rm -f "${DEST}"
fi

cp "${TMP_DIR}/${BINARY}-${VERSION}-${TARGET}/${BINARY}" "${DEST}"
chmod +x "${DEST}"

echo ""
echo "Installed: ${DEST}"

# ── PATH check ────────────────────────────────────────────────────────────────

if ! echo ":${PATH}:" | grep -q ":${INSTALL_DIR}:"; then
    echo ""
    echo "Note: ${INSTALL_DIR} is not in your PATH."
    echo "Add the following to your shell profile (~/.bashrc, ~/.zshrc, etc.):"
    echo ""
    echo "    export PATH=\"${INSTALL_DIR}:\$PATH\""
    echo ""
fi

echo "Done. Run 'delta --help' to get started."
