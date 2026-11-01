#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP_PATH="$ROOT_DIR/artifacts/ReShift.app"

if [ ! -d "$APP_PATH" ]; then
    echo "App bundle not found at $APP_PATH. Building first..."
    "$ROOT_DIR/scripts/bundle.sh"
fi

echo "=========================================================="
echo "         ReShift macOS E2E Smoke Test"
echo "=========================================================="
echo "NOTE: This test launches the bundled app and uses TextEdit"
echo "to test real conversions via simulated keyboard events."
echo "Please do not use the keyboard/mouse for ~15 seconds."
echo "=========================================================="
sleep 2

# Cleanup previous instances
pkill -f "ReShift" 2>/dev/null || true
osascript -e 'tell application "TextEdit" to quit saving no' 2>/dev/null || true
sleep 1

LOG_FILE="/tmp/reshift_e2e.log"
rm -f "$LOG_FILE"

# Launch bundled application
"$APP_PATH/Contents/MacOS/ReShift" --log-file "$LOG_FILE" &
APP_PID=$!

cleanup() {
    kill "$APP_PID" 2>/dev/null || true
    wait "$APP_PID" 2>/dev/null || true
    osascript -e 'tell application "TextEdit" to quit saving no' 2>/dev/null || true
}
trap cleanup EXIT

sleep 2
echo "App running with PID: $APP_PID"

# Compile helper poster if needed
POSTER_BIN="/tmp/reshift_poster"
cat > /tmp/reshift_poster.c << 'EOF'
#include <ApplicationServices/ApplicationServices.h>
#include <unistd.h>
int main() {
    CGEventSourceRef src = CGEventSourceCreate(kCGEventSourceStateHIDSystemState);
    for (int i = 0; i < 2; i++) {
        CGEventRef down = CGEventCreateKeyboardEvent(src, 0x38, true);
        CGEventSetType(down, kCGEventFlagsChanged);
        CGEventSetFlags(down, kCGEventFlagMaskShift);
        CGEventPost(kCGHIDEventTap, down);
        CFRelease(down);
        usleep(30000);
        CGEventRef up = CGEventCreateKeyboardEvent(src, 0x38, false);
        CGEventSetType(up, kCGEventFlagsChanged);
        CGEventSetFlags(up, 0);
        CGEventPost(kCGHIDEventTap, up);
        CFRelease(up);
        usleep(60000);
    }
    CFRelease(src);
    usleep(300000);
    return 0;
}
EOF
clang /tmp/reshift_poster.c -framework ApplicationServices -o "$POSTER_BIN"

# --- Scenario 1: EN -> RU Smart Conversion ---
echo "--- Scenario 1: EN -> RU Smart Conversion (ghbdtn -> привет) ---"
osascript << 'EOF'
tell application "TextEdit"
    activate
    make new document with properties {text:"ghbdtn"}
end tell
delay 1
tell application "System Events"
    tell process "TextEdit"
        set frontmost to true
        try
            click text area 1 of scroll area 1 of window 1
        end try
        delay 0.6
        key code 124 using command down
    end tell
end tell
EOF
sleep 1

"$POSTER_BIN"
sleep 2

RES_1=$(osascript -e 'tell application "TextEdit" to return text of document 1' | tr -d '\r\n')
echo "Result 1: '$RES_1'"
if [ "$RES_1" != "привет" ]; then
    echo "FAIL: Expected 'привет', got '$RES_1'"
    exit 1
fi
echo "PASS: Scenario 1 succeeded!"

# --- Scenario 2: RU -> EN Smart Conversion ---
echo ""
echo "--- Scenario 2: RU -> EN Smart Conversion (руддщ -> hello) ---"
osascript -e 'tell application "TextEdit" to close document 1 saving no'
sleep 2.0
osascript << 'EOF'
tell application "TextEdit"
    activate
    make new document with properties {text:"руддщ"}
end tell
delay 1
tell application "System Events"
    tell process "TextEdit"
        set frontmost to true
        try
            click text area 1 of scroll area 1 of window 1
        end try
        delay 0.6
        key code 124 using command down
    end tell
end tell
EOF
sleep 1

"$POSTER_BIN"
sleep 2

RES_2=$(osascript -e 'tell application "TextEdit" to return text of document 1' | tr -d '\r\n')
echo "Result 2: '$RES_2'"
if [ "$RES_2" != "hello" ]; then
    echo "FAIL: Expected 'hello', got '$RES_2'"
    exit 1
fi
echo "PASS: Scenario 2 succeeded!"

# --- Scenario 3: Clipboard Preservation ---
echo ""
echo "--- Scenario 3: Clipboard snapshot & restore ---"
pbcopy << 'EOF'
MY_ORIGINAL_CLIPBOARD_CONTENT
EOF
sleep 0.5

osascript -e 'tell application "TextEdit" to close document 1 saving no'
sleep 2.0
osascript << 'EOF'
tell application "TextEdit"
    activate
    make new document with properties {text:"rfr ltkf"}
end tell
delay 1
tell application "System Events"
    tell process "TextEdit"
        set frontmost to true
        try
            click text area 1 of scroll area 1 of window 1
        end try
        delay 0.6
        key code 124 using command down
    end tell
end tell
EOF
sleep 1

"$POSTER_BIN"
sleep 2

# Check conversion
RES_3=$(osascript -e 'tell application "TextEdit" to return text of document 1' | tr -d '\r\n')
echo "Converted: '$RES_3'"
if [ "$RES_3" != "как дела" ]; then
    echo "FAIL: Expected 'как дела', got '$RES_3'"
    exit 1
fi

# Check clipboard restored after 300ms
CLIP_RESTORED=$(pbpaste | tr -d '\r\n')
echo "Clipboard content now: '$CLIP_RESTORED'"
if [ "$CLIP_RESTORED" != "MY_ORIGINAL_CLIPBOARD_CONTENT" ]; then
    echo "FAIL: Clipboard was not restored! Got: '$CLIP_RESTORED'"
    exit 1
fi
echo "PASS: Scenario 3 succeeded (clipboard perfectly restored)!"

echo ""
echo "=========================================================="
echo "               ALL E2E SMOKE TESTS PASSED!                "
echo "=========================================================="
