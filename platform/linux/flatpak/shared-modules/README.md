# Vendored Flatpak shared modules

The `intltool` and `libayatana-appindicator` module definitions and patches are
vendored from `flathub/shared-modules` commit
`7b858d89ffe3bf9ce6e0390fe72691c9c5f322d3`. The appindicator definition was
reformatted and its update-checker-only metadata removed; all build source
URLs, archive hashes, tags, commits, options, and patches remain pinned.

These build-only inputs provide the GTK 3 AppIndicator runtime required by
Tauri's tray implementation. They are fetched by `flatpak-builder` before the
offline sandbox build. Runtime D-Bus access remains limited to the
`org.kde.StatusNotifierWatcher` registration service.
