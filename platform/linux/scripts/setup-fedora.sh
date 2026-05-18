#!/usr/bin/env bash
set -euo pipefail

sudo dnf install -y \
    @development-tools curl pkg-config \
    webkit2gtk4.1-devel gtk3-devel libsoup3-devel javascriptcoregtk4.1-devel \
    wayland-devel libxkbcommon-devel wayland-protocols-devel \
    libxcb-devel libXcomposite-devel libXfixes-devel libXShape-devel \
    pipewire-devel \
    vulkan-loader-devel vulkan-headers mesa-vulkan-drivers

if ! command -v rustup >/dev/null; then
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source "$HOME/.cargo/env"
fi

cargo install tauri-cli --version "^2.0" --locked
echo "✓ Fedora deps installed."
