#!/usr/bin/env bash
# Install LiveBlock build prereqs on Ubuntu 24.04+ / Debian Trixie+.
set -euo pipefail

if ! command -v sudo >/dev/null; then
    echo "This script needs sudo." >&2
    exit 1
fi

echo "→ apt install build deps…"
sudo apt update
sudo apt install -y \
    build-essential pkg-config curl \
    libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev \
    libjavascriptcoregtk-4.1-dev \
    libwayland-dev libxkbcommon-dev wayland-protocols \
    libxcb-shm0-dev libxcb-composite0-dev libxcb-shape0-dev libxcb-xfixes0-dev \
    libpipewire-0.3-dev \
    libvulkan-dev mesa-vulkan-drivers

if ! command -v rustup >/dev/null; then
    echo "→ installing rustup…"
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source "$HOME/.cargo/env"
fi

echo "→ tauri CLI…"
cargo install tauri-cli --version "^2.0" --locked

echo "✓ Ubuntu/Debian deps installed. Now: cd platform/linux && npm install && cargo tauri dev"
