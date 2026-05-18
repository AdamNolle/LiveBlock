#!/usr/bin/env bash
# Build the liveblock-bridge static lib (universal arm64 + x86_64) and emit
# Swift glue + C header into `core/crates/liveblock-bridge/generated/`.
#
# Usage:
#   tools/build_bridge.sh                  # debug, universal
#   LB_BRIDGE_PROFILE=release tools/build_bridge.sh
#   CONFIGURATION=Debug tools/build_bridge.sh   # honors Xcode env
#   LB_BRIDGE_HOST_ONLY=1 tools/build_bridge.sh # skip x86_64 cross-compile
#
# Output:
#   core/target/universal/<profile>/libliveblock_bridge.a — fat archive
#   core/crates/liveblock-bridge/generated/                — Swift + C glue

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CRATE_ROOT="$REPO_ROOT/core"

# Honor Xcode's $CONFIGURATION when invoked as a build phase. Default debug.
if [[ -n "${CONFIGURATION:-}" ]]; then
  case "$CONFIGURATION" in
    Release) PROFILE="release" ;;
    *)       PROFILE="debug" ;;
  esac
else
  PROFILE="${LB_BRIDGE_PROFILE:-debug}"
fi

CARGO_PROFILE_FLAG=()
if [[ "$PROFILE" == "release" ]]; then
  CARGO_PROFILE_FLAG=(--release)
fi

# Build for both Apple Silicon and Intel so the resulting `.a` is universal.
# Skippable via LB_BRIDGE_HOST_ONLY=1 when iterating quickly.
TARGETS=(aarch64-apple-darwin)
if [[ -z "${LB_BRIDGE_HOST_ONLY:-}" ]]; then
  TARGETS+=(x86_64-apple-darwin)
fi

echo "==> ensuring rust targets are installed"
for t in "${TARGETS[@]}"; do
  rustup target add "$t" >/dev/null 2>&1 || true
done

for t in "${TARGETS[@]}"; do
  echo "==> cargo build -p liveblock-bridge --target $t ($PROFILE)"
  (
    cd "$CRATE_ROOT"
    cargo build -p liveblock-bridge --target "$t" ${CARGO_PROFILE_FLAG[@]+"${CARGO_PROFILE_FLAG[@]}"}
  )
done

UNIVERSAL_DIR="$CRATE_ROOT/target/universal/$PROFILE"
mkdir -p "$UNIVERSAL_DIR"
UNIVERSAL_LIB="$UNIVERSAL_DIR/libliveblock_bridge.a"

LIPO_INPUTS=()
for t in "${TARGETS[@]}"; do
  LIPO_INPUTS+=("$CRATE_ROOT/target/$t/$PROFILE/libliveblock_bridge.a")
done

echo "==> lipo -create -> $UNIVERSAL_LIB"
if [[ "${#LIPO_INPUTS[@]}" -eq 1 ]]; then
  cp "${LIPO_INPUTS[0]}" "$UNIVERSAL_LIB"
else
  lipo -create "${LIPO_INPUTS[@]}" -output "$UNIVERSAL_LIB"
fi

GENERATED="$CRATE_ROOT/crates/liveblock-bridge/generated"

echo "==> generated Swift glue:"
ls -1 "$GENERATED" || true

echo "==> universal static lib:"
ls -lh "$UNIVERSAL_LIB"
lipo -info "$UNIVERSAL_LIB" || true
