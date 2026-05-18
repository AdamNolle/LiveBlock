#!/usr/bin/env bash
# Build and run LiveBlock from the terminal.
#
# Usage:
#   ./run.sh                 # regenerate, build, launch from DerivedData
#   ./run.sh --clean         # also wipe DerivedData first
#   ./run.sh --test          # run tests instead of launching the app
#   ./run.sh --build         # build only, do not launch
#   ./run.sh --install       # build, then copy .app to /Applications and launch from there
#   ./run.sh --watch         # rebuild + relaunch on every save under Sources/, core/, project.yml
#   ./run.sh --reset-perms   # print the tccutil commands that wipe TCC grants for this app
#
# First-time setup: run `tools/setup_codesign_identity.sh` once. That
# writes tools/.codesign_identity with a stable codesigning identity so
# macOS TCC remembers the Screen Recording / Accessibility grants across
# rebuilds (otherwise every Debug build is signed adhoc and TCC re-prompts).

set -euo pipefail

cd "$(dirname "$0")"

SCHEME="LiveBlock"
PROJECT="LiveBlock.xcodeproj"
BUNDLE_ID="com.adamnolle.LiveBlock"
CONFIG="Debug"
DESTINATION='platform=macOS'
DERIVED="$(cd ~/Library/Developer/Xcode/DerivedData 2>/dev/null && pwd || true)"
INSTALL_DIR="/Applications"

CLEAN=0
RUN_TESTS=0
LAUNCH=1
INSTALL=0
WATCH=0
RESET_PERMS=0
for arg in "$@"; do
    case "$arg" in
        --clean) CLEAN=1 ;;
        --test|--tests) RUN_TESTS=1; LAUNCH=0 ;;
        --build) LAUNCH=0 ;;
        --install) INSTALL=1 ;;
        --watch) WATCH=1 ;;
        --reset-perms) RESET_PERMS=1 ;;
        -h|--help)
            sed -n '2,17p' "$0"
            exit 0
            ;;
        *) echo "Unknown flag: $arg" >&2; exit 2 ;;
    esac
done

# --reset-perms: print, don't execute. Wiping TCC requires sudo + admin trust;
# the user must run these explicitly. Do this BEFORE the project guards so the
# user can recover even when the working tree is in a confused state.
if [[ $RESET_PERMS -eq 1 ]]; then
    cat <<EOF
To reset macOS permission grants for LiveBlock, copy and paste:

    sudo tccutil reset ScreenCapture $BUNDLE_ID
    sudo tccutil reset Accessibility $BUNDLE_ID

Then delete any stale installed copy and rebuild:

    rm -rf "$INSTALL_DIR/$SCHEME.app"
    ./run.sh

First launch will reprompt for Screen Recording and Accessibility. Grant both;
subsequent rebuilds will keep the grants because the codesign hash is stable.
EOF
    exit 0
fi

# Stale .xcodeproj guard: Finder duplicates ("LiveBlock 2.xcodeproj") confuse
# both Xcode and TCC (parallel cdhashes). Refuse to run; don't auto-delete.
shopt -s nullglob
STALE_PROJECTS=( "LiveBlock "*".xcodeproj" )
shopt -u nullglob
if [[ ${#STALE_PROJECTS[@]} -gt 0 ]]; then
    echo "Found stale Xcode project(s) alongside the canonical $PROJECT:" >&2
    for p in "${STALE_PROJECTS[@]}"; do echo "  - $p" >&2; done
    cat >&2 <<EOF

These are usually Finder duplicate-paste artifacts. They cause TCC permissions
to drop intermittently because macOS sees each one as a separate app. The
canonical project is regenerated from project.yml by xcodegen, so the
duplicate(s) are safe to delete:

    rm -rf "${STALE_PROJECTS[@]}"

Then re-run ./run.sh.
EOF
    exit 1
fi

step() { printf '\n\033[1;34m▸ %s\033[0m\n' "$*"; }

# 1. Tooling check
command -v xcodegen >/dev/null || { echo "xcodegen not found. Install: brew install xcodegen" >&2; exit 1; }
command -v xcodebuild >/dev/null || { echo "xcodebuild not found. Install Xcode command-line tools." >&2; exit 1; }

# 1a. Source the codesigning identity if setup_codesign_identity.sh has run.
#     Two paths:
#       - Apple-issued cert: pass DEVELOPMENT_TEAM only and let Xcode's
#         Automatic signing pick the matching cert from the keychain.
#         (Setting CODE_SIGN_IDENTITY explicitly while Automatic is on is
#         rejected as a "conflicting provisioning settings" error.)
#       - Self-signed cert: switch to Manual signing and name it explicitly.
#     Falls back to adhoc ("Sign to Run Locally") if the file doesn't exist.
SIGN_ARGS=()
if [[ -f tools/.codesign_identity ]]; then
    # shellcheck disable=SC1091
    source tools/.codesign_identity
    if [[ -n "${LB_DEVELOPMENT_TEAM:-}" ]]; then
        # Apple-issued cert: switch to Manual signing so we pick the exact
        # cert by name. CODE_SIGN_IDENTITY here is the generic family name
        # ("Apple Development") — xcodebuild resolves it to the matching
        # private key in the login keychain via DEVELOPMENT_TEAM.
        SIGN_ARGS+=(
            "CODE_SIGN_STYLE=Manual"
            "CODE_SIGN_IDENTITY=Apple Development"
            "DEVELOPMENT_TEAM=${LB_DEVELOPMENT_TEAM}"
            "CODE_SIGN_ENTITLEMENTS="
        )
    elif [[ -n "${LB_CODESIGN_IDENTITY:-}" ]]; then
        SIGN_ARGS+=(
            "CODE_SIGN_STYLE=Manual"
            "CODE_SIGN_IDENTITY=${LB_CODESIGN_IDENTITY}"
        )
    fi
fi

# 2. Optional clean
if [[ $CLEAN -eq 1 ]]; then
    step "Cleaning DerivedData"
    if [[ -n "${DERIVED:-}" ]]; then
        rm -rf "$DERIVED"/LiveBlock-*
    fi
    rm -rf build
fi

# 3. Regenerate xcodeproj from project.yml
step "Regenerating $PROJECT (xcodegen)"
xcodegen generate

# 4. Build (or test)
if [[ $RUN_TESTS -eq 1 ]]; then
    step "Running tests"
    xcodebuild \
        -project "$PROJECT" \
        -scheme "$SCHEME" \
        -destination "$DESTINATION" \
        -configuration "$CONFIG" \
        "${SIGN_ARGS[@]}" \
        test \
        | xcbeautify 2>/dev/null || \
    xcodebuild \
        -project "$PROJECT" \
        -scheme "$SCHEME" \
        -destination "$DESTINATION" \
        -configuration "$CONFIG" \
        "${SIGN_ARGS[@]}" \
        test
    exit $?
fi

step "Building $SCHEME ($CONFIG)"
xcodebuild \
    -project "$PROJECT" \
    -scheme "$SCHEME" \
    -destination "$DESTINATION" \
    -configuration "$CONFIG" \
    "${SIGN_ARGS[@]}" \
    build \
    | (xcbeautify 2>/dev/null || cat) \
    | grep -v -E '^\s*(SwiftDriver|builtin-|/Applications/Xcode|cd )' \
    | tail -40

# 5. Locate the built .app
APP_PATH=$(xcodebuild \
    -project "$PROJECT" \
    -scheme "$SCHEME" \
    -configuration "$CONFIG" \
    -showBuildSettings 2>/dev/null \
    | awk -F' = ' '/^[[:space:]]*BUILT_PRODUCTS_DIR =/ {print $2; exit}')
APP="$APP_PATH/$SCHEME.app"

if [[ ! -d "$APP" ]]; then
    echo "Build appears to have failed: $APP not found" >&2
    exit 1
fi

step "Built: $APP"

# 5a. Optional install to /Applications. `ditto` preserves extended attrs +
#     code signature so the installed copy keeps its TCC grant. After copy,
#     strip the quarantine xattr so Gatekeeper doesn't block first launch
#     (the self-signed cert isn't on Apple's trust list, so without this
#     step macOS shows "Apple cannot verify..." on every double-click).
if [[ $INSTALL -eq 1 ]]; then
    DEST="$INSTALL_DIR/$SCHEME.app"
    step "Installing to $DEST"
    rm -rf "$DEST"
    ditto "$APP" "$DEST"
    xattr -dr com.apple.quarantine "$DEST" 2>/dev/null || true
    spctl --add --label "LiveBlocker Local" "$DEST" 2>/dev/null || true
    /System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister \
        -f "$DEST" >/dev/null 2>&1 || true
    APP="$DEST"
    echo "Installed. Launch from /Applications, the Dock, or Spotlight."
fi

# 5b. Stale /Applications warning. We launch from DerivedData (or, with
#     --install, from /Applications). If a /Applications copy exists with a
#     different cdhash than what we just built, the Dock/Spotlight icon will
#     open the wrong binary and TCC will think it's a different app. Warn
#     loudly so the user knows to use --install or rm /Applications/<app>.
if [[ $INSTALL -eq 0 && -d "$INSTALL_DIR/$SCHEME.app" ]]; then
    BUILT_HASH=$(codesign -dvvv "$APP" 2>&1 | awk -F'=' '/CDHash/ {print $2; exit}')
    INSTALLED_HASH=$(codesign -dvvv "$INSTALL_DIR/$SCHEME.app" 2>&1 | awk -F'=' '/CDHash/ {print $2; exit}')
    if [[ -n "${BUILT_HASH:-}" && -n "${INSTALLED_HASH:-}" && "$BUILT_HASH" != "$INSTALLED_HASH" ]]; then
        printf '\033[1;33m⚠  /Applications/%s.app has a different code-signing hash than the fresh build.\033[0m\n' "$SCHEME"
        echo "   Dock / Spotlight / double-click will launch the STALE copy and TCC may"
        echo "   refuse permissions on the new build. To fix:"
        echo "     ./run.sh --install      # refresh /Applications with the new build"
        echo "   or"
        echo "     rm -rf '$INSTALL_DIR/$SCHEME.app'"
    fi
fi

# 6. Launch (unless --build)
if [[ $LAUNCH -eq 1 ]]; then
    pkill -x LiveBlock 2>/dev/null || true
    step "Launching"

    # Record the repo root so the in-app training dashboard can locate
    # tools/.venv, tools/auto.sh, etc. (LaunchServices doesn't propagate env
    # variables, so we use a small config file instead.)
    CONFIG_DIR="$HOME/Library/Application Support/LiveBlock"
    mkdir -p "$CONFIG_DIR"
    pwd > "$CONFIG_DIR/repo_path.txt"

    # Force LaunchServices to re-read the bundle (pick up the new icon /
    # Info.plist) and bounce the Dock so the cached icon refreshes.
    touch "$APP"
    /System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister \
        -f "$APP" >/dev/null 2>&1 || true
    killall Dock 2>/dev/null || true
    killall Finder 2>/dev/null || true

    open -n "$APP"
    if [[ -f tools/.codesign_identity ]]; then
        echo "LiveBlocker launched (signed with $LB_CODESIGN_IDENTITY)."
        echo "Screen Recording / Accessibility grants persist across rebuilds."
    else
        echo "LiveBlocker launched."
        echo "Tip: run tools/setup_codesign_identity.sh once so TCC keeps your permission grants across rebuilds."
    fi
fi

# 7. Watch mode — re-run on every save under Sources/, core/, project.yml.
#    Recurses into ./run.sh (without --watch) so all the same guards apply.
if [[ $WATCH -eq 1 ]]; then
    if ! command -v fswatch >/dev/null; then
        echo "" >&2
        echo "--watch needs fswatch. Install with: brew install fswatch" >&2
        exit 1
    fi
    step "Watching Sources/, core/, project.yml — Ctrl+C to stop"
    # `--latency 0.4` debounces multi-file saves into one rebuild.
    # `--one-per-batch` collapses bursts into a single event.
    fswatch -r -o --latency 0.4 --one-per-batch \
        --exclude '\.git' \
        --exclude 'DerivedData' \
        --exclude '\.xcodeproj' \
        --exclude 'core/target' \
        Sources core project.yml 2>/dev/null | while read -r _; do
        echo ""
        echo "▸ Change detected — rebuilding"
        if ! "$0"; then
            echo "Rebuild failed. Waiting for next change..." >&2
        fi
    done
fi
