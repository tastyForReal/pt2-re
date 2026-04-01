#!/usr/bin/env bash
set -euo pipefail

# =============================================================================
# pt2-re build script
# Builds the project with the "soundfont" feature enabled.
# Requires: curl, a C compiler (cc), make, and internet access.
# =============================================================================

echo "==> Installing Rust toolchain..."
# Ensure we're in the project root (where this script lives)
cd "$(dirname "$0")"

curl -fsSL https://sh.rustup.rs | sh -s -- -y --default-toolchain=stable --profile=minimal
. "$HOME/.cargo/env"
rustup component add rustfmt clippy

echo "==> Cleaning cargo registry cache..."
rm -rf "$HOME/.cargo/registry"/*

# =============================================================================
# ALSA development library (libasound2-dev)
# Required by the rodio -> cpal -> alsa-sys dependency chain.
# Try installing via package manager; fall back to manual download+extract.
# =============================================================================
ALSA_DEV_DIR="/tmp/alsa-dev"
install_alsa_dev() {
    echo "==> Installing ALSA development library..."
    if command -v apt-get &>/dev/null; then
        # Try with first
        if apt-get update -qq && apt-get install -y -qq libasound2-dev pkg-config 2>/dev/null; then
            echo "    Installed libasound2-dev via apt-get (sudo)."
            return 0
        fi
        # Fall back: download the .deb manually and extract it
        echo "    unavailable — downloading libasound2-dev .deb manually..."
        rm -f libasound2-dev_*.deb
        if apt download libasound2-dev 2>/dev/null; then
            deb=$(ls libasound2-dev_*.deb 2>/dev/null | head -1)
            if [ -n "$deb" ] && [ -f "$deb" ]; then
                rm -rf "$ALSA_DEV_DIR"
                mkdir -p "$ALSA_DEV_DIR"
                dpkg -x "$deb" "$ALSA_DEV_DIR"
                rm -f "$deb"
                echo "    Extracted libasound2-dev to $ALSA_DEV_DIR"
                return 0
            fi
        fi
        echo "WARNING: Failed to install ALSA dev library. Linking may fail."
        return 1
    elif command -v dnf &>/dev/null; then
        if dnf install -y alsa-lib-devel 2>/dev/null; then
            echo "    Installed alsa-lib-devel via dnf."
            return 0
        fi
    elif command -v pacman &>/dev/null; then
        if pacman -S --noconfirm alsa-lib 2>/dev/null; then
            echo "    Installed alsa-lib via pacman."
            return 0
        fi
    fi
    echo "WARNING: No supported package manager found for ALSA dev library."
    return 1
}

install_alsa_dev

# =============================================================================
# Set up environment for ALSA (needed if we fell back to manual extraction)
# =============================================================================
if [ -d "$ALSA_DEV_DIR/usr/lib/x86_64-linux-gnu/pkgconfig" ]; then
    export PKG_CONFIG_PATH="$ALSA_DEV_DIR/usr/lib/x86_64-linux-gnu/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
fi
if [ -d "$ALSA_DEV_DIR/usr/include" ]; then
    export C_INCLUDE_PATH="$ALSA_DEV_DIR/usr/include${C_INCLUDE_PATH:+:$C_INCLUDE_PATH}"
fi

# Make sure the linker can find libasound
# On systems where only the runtime .so.2 is present (no .so symlink),
# create a symlink in a writable directory and pass it via RUSTFLAGS.
if ! ldconfig -p 2>/dev/null | grep -q "libasound.so "; then
    LINKER_LIB_DIR="$PWD/lib"
    mkdir -p "$LINKER_LIB_DIR"
    if [ -f "$ALSA_DEV_DIR/usr/lib/x86_64-linux-gnu/libasound.so.2.0.0" ]; then
        ln -sf "$ALSA_DEV_DIR/usr/lib/x86_64-linux-gnu/libasound.so.2.0.0" "$LINKER_LIB_DIR/libasound.so"
    elif [ -f "/usr/lib/x86_64-linux-gnu/libasound.so.2.0.0" ]; then
        ln -sf /usr/lib/x86_64-linux-gnu/libasound.so.2.0.0 "$LINKER_LIB_DIR/libasound.so"
    fi
    export RUSTFLAGS="-L $LINKER_LIB_DIR${RUSTFLAGS:+ $RUSTFLAGS}"
    echo "    Created libasound.so symlink at $LINKER_LIB_DIR/libasound.so"
fi

# =============================================================================
# Build
# =============================================================================
echo "==> Building pt2-re (soundfont feature)..."
cargo build --features "soundfont"

echo "==> Running cargo fmt"
cargo fmt

echo "==> Running cargo clippy -- -D \"warnings\""
cargo clippy -- -D "warnings"

echo "==> Build complete."