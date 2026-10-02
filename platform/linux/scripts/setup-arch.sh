#!/usr/bin/env bash
set -euo pipefail

sudo pacman -S --needed --noconfirm \
    base-devel curl pkg-config \
    webkit2gtk-4.1 gtk3 libsoup3 \
    wayland libxkbcommon wayland-protocols \
    libxcomposite libxfixes libxcb \
    pipewire pipewire-jack \
    vulkan-icd-loader vulkan-headers vulkan-mesa-layers \
    nodejs npm

if ! command -v rustup >/dev/null; then
    sudo pacman -S --needed --noconfirm rustup
    rustup default stable
fi

cargo install tauri-cli --version "^2.0" --locked
echo "✓ Arch deps installed."
