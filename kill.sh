#!/usr/bin/env bash
# Emergency kill for LiveBlock. Try graceful first, then escalate.
# Always exits 0; running this multiple times is safe.

GRACEFUL_SLEEP=0.4

# 1. Polite quit via AppleScript.
osascript -e 'tell application "LiveBlock" to quit' >/dev/null 2>&1 || true
sleep "$GRACEFUL_SLEEP"

# 2. SIGTERM by exact name (NSStatusItem and windows clean up).
pkill -x LiveBlock >/dev/null 2>&1 || true
sleep "$GRACEFUL_SLEEP"

# 3. SIGKILL if still alive.
pkill -KILL -x LiveBlock >/dev/null 2>&1 || true
killall -KILL LiveBlock >/dev/null 2>&1 || true

if pgrep -x LiveBlock >/dev/null 2>&1; then
    echo "❌ LiveBlock is still running. Try Activity Monitor → search 'LiveBlock' → Force Quit."
    exit 1
fi
echo "✓ LiveBlock is not running."
