# Pinned NSIS template

`installer.nsi` is the Tauri NSIS template pinned to the exact source used by
`@tauri-apps/cli` 2.11.1:

- Repository: <https://github.com/tauri-apps/tauri>
- Commit/tag: `e5ae5b93cdd310045191cc0526f253140ad64b87`
  (`tauri-cli-v2.11.1` / `tauri-bundler-v2.9.1`)
- Upstream path: `crates/tauri-bundler/src/bundle/windows/nsis/installer.nsi`
- Upstream SHA-256: `ee84148e405adc4d736a46456dd8345a644751bd1f28a335dd7fd833a32d7c3e`

LiveBlock adds one marked `LIVEBLOCK BEGIN/END DOWNGRADE GUARD` block in
`.onInit`. Tauri's stock silent path can reach `EarlyChecks` without the custom
reinstall page having populated `$R0`, allowing an older silent fixture to
replace the registered current version despite `allowDowngrades: false`. The
custom block reads `DisplayVersion` before any uninstall or payload mutation,
uses Tauri's bundled semantic-version comparator, and aborts older or malformed
incoming comparisons with a nonzero process exit code. Same-version reinstall
and upgrades remain allowed.

`tools/test_windows_packaging.py` removes exactly that marked block in memory
and requires the remaining bytes to match the upstream SHA-256. Any upstream
refresh must re-pin the Tauri CLI, review the full template diff, update this
record, and rerun the hosted prior/current NSIS lifecycle.
