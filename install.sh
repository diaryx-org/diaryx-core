#!/bin/bash
#
# Diaryx Install Script
# 
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/diaryx-org/diaryx-core/main/install.sh | bash
#
# Options (via environment variables):
#   DIARYX_VERSION  - Specific version to install (default: latest)
#   DIARYX_INSTALL_DIR - Installation directory (default: ~/.local/bin)
#

set -euo pipefail

# Configuration
REPO="diaryx-org/diaryx-core"
BINARY_NAME="diaryx"
DEFAULT_INSTALL_DIR="${HOME}/.local/bin"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Logging functions
info() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

success() {
    echo -e "${GREEN}[SUCCESS]${NC} $1"
}

warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

error() {
    echo -e "${RED}[ERROR]${NC} $1" >&2
    exit 1
}

# Detect OS and architecture
detect_platform() {
    local os arch

    case "$(uname -s)" in
        Linux*)     os="unknown-linux-gnu" ;;
        Darwin*)    os="apple-darwin" ;;
        MINGW*|MSYS*|CYGWIN*)
            os="pc-windows-msvc"
            ;;
        *)
            error "Unsupported operating system: $(uname -s)"
            ;;
    esac

    case "$(uname -m)" in
        x86_64|amd64)   arch="x86_64" ;;
        aarch64|arm64)  arch="aarch64" ;;
        *)
            error "Unsupported architecture: $(uname -m)"
            ;;
    esac

    echo "${arch}-${os}"
}

# Get the latest release version from GitHub
get_latest_version() {
    local latest_url="https://api.github.com/repos/${REPO}/releases/latest"
    
    if command -v curl &> /dev/null; then
        curl -fsSL "$latest_url" | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/'
    elif command -v wget &> /dev/null; then
        wget -qO- "$latest_url" | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/'
    else
        error "Neither curl nor wget found. Please install one of them."
    fi
}

# Download file
download() {
    local url="$1"
    local output="$2"
    
    info "Downloading from: $url"
    
    if command -v curl &> /dev/null; then
        curl -fsSL "$url" -o "$output"
    elif command -v wget &> /dev/null; then
        wget -q "$url" -O "$output"
    else
        error "Neither curl nor wget found. Please install one of them."
    fi
}

# Verify checksum
verify_checksum() {
    local file="$1"
    local expected="$2"
    
    local actual
    if command -v sha256sum &> /dev/null; then
        actual=$(sha256sum "$file" | awk '{print $1}')
    elif command -v shasum &> /dev/null; then
        actual=$(shasum -a 256 "$file" | awk '{print $1}')
    else
        warn "Neither sha256sum nor shasum found. Skipping checksum verification."
        return 0
    fi
    
    if [ "$actual" != "$expected" ]; then
        error "Checksum verification failed!\n  Expected: $expected\n  Actual:   $actual"
    fi
    
    info "Checksum verified successfully"
}

# Main installation function
main() {
    echo ""
    echo "╔══════════════════════════════════════╗"
    echo "║       Diaryx Installer               ║"
    echo "╚══════════════════════════════════════╝"
    echo ""

    # Detect platform
    local platform
    platform=$(detect_platform)
    info "Detected platform: $platform"

    # Determine version
    local version="${DIARYX_VERSION:-}"
    if [ -z "$version" ]; then
        info "Fetching latest release version..."
        version=$(get_latest_version)
        if [ -z "$version" ]; then
            error "Failed to determine latest version"
        fi
    fi
    info "Installing version: $version"

    # Determine install directory
    local install_dir="${DIARYX_INSTALL_DIR:-$DEFAULT_INSTALL_DIR}"
    info "Install directory: $install_dir"

    # Create temp directory
    local tmp_dir
    tmp_dir=$(mktemp -d)
    trap 'rm -rf "$tmp_dir"' EXIT

    # Determine archive extension
    local archive_ext="tar.gz"
    if [[ "$platform" == *"windows"* ]]; then
        archive_ext="zip"
    fi

    # Download URLs
    local archive_name="${BINARY_NAME}-${platform}.${archive_ext}"
    local download_url="https://github.com/${REPO}/releases/download/${version}/${archive_name}"
    local checksum_url="https://github.com/${REPO}/releases/download/${version}/SHA256SUMS.txt"

    # Download archive
    local archive_path="${tmp_dir}/${archive_name}"
    download "$download_url" "$archive_path"

    # Download and verify checksum
    local checksum_path="${tmp_dir}/SHA256SUMS.txt"
    download "$checksum_url" "$checksum_path"
    
    local expected_checksum
    expected_checksum=$(grep "$archive_name" "$checksum_path" | awk '{print $1}')
    if [ -n "$expected_checksum" ]; then
        verify_checksum "$archive_path" "$expected_checksum"
    else
        warn "Could not find checksum for $archive_name. Skipping verification."
    fi

    # Extract archive
    info "Extracting archive..."
    local extract_dir="${tmp_dir}/extracted"
    mkdir -p "$extract_dir"
    
    if [[ "$archive_ext" == "tar.gz" ]]; then
        tar -xzf "$archive_path" -C "$extract_dir"
    else
        # For Windows (zip)
        if command -v unzip &> /dev/null; then
            unzip -q "$archive_path" -d "$extract_dir"
        else
            error "unzip not found. Please install it to extract the archive."
        fi
    fi

    # Create install directory if needed
    mkdir -p "$install_dir"

    # Install binary
    local binary_path="${extract_dir}/${BINARY_NAME}"
    if [[ "$platform" == *"windows"* ]]; then
        binary_path="${extract_dir}/${BINARY_NAME}.exe"
    fi

    if [ ! -f "$binary_path" ]; then
        error "Binary not found in archive: $binary_path"
    fi

    info "Installing ${BINARY_NAME} to ${install_dir}..."
    cp "$binary_path" "$install_dir/"
    chmod +x "${install_dir}/${BINARY_NAME}"

    success "Diaryx ${version} installed successfully!"
    echo ""

    # Check if install_dir is in PATH
    if [[ ":$PATH:" != *":${install_dir}:"* ]]; then
        warn "Installation directory is not in your PATH."
        echo ""
        echo "Add the following to your shell configuration file (~/.bashrc, ~/.zshrc, etc.):"
        echo ""
        echo "  export PATH=\"\$PATH:${install_dir}\""
        echo ""
        echo "Then reload your shell or run:"
        echo ""
        echo "  source ~/.bashrc  # or ~/.zshrc"
        echo ""
    fi

    # Verify installation
    if command -v "${install_dir}/${BINARY_NAME}" &> /dev/null; then
        echo "Installed version:"
        "${install_dir}/${BINARY_NAME}" --version 2>/dev/null || echo "  ${BINARY_NAME} ${version}"
    fi

    echo ""
    echo "To get started, run:"
    echo ""
    echo "  ${BINARY_NAME} init"
    echo ""
}

# Run main function
main "$@"