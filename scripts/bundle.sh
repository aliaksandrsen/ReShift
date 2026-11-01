#!/usr/bin/env bash
set -euo pipefail

# Scripts directory
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

BUNDLE_ID="com.reshift.rust"
ADHOC=false

for arg in "$@"; do
    case "$arg" in
        --dev)
            BUNDLE_ID="com.reshift.rust.dev"
            ;;
        --adhoc)
            ADHOC=true
            ;;
        *)
            echo "Unknown argument: $arg"
            echo "Usage: $0 [--dev] [--adhoc]"
            exit 1
            ;;
    esac
done

echo "=== Building ReShift (Release) ==="
cargo build --release -p reshift

VERSION=$(grep -m1 '^version = ' Cargo.toml | cut -d '"' -f2)
echo "Version: $VERSION, Bundle ID: $BUNDLE_ID"

ARTIFACTS_DIR="$ROOT_DIR/artifacts"
APP_NAME="ReShift.app"
APP_DIR="$ARTIFACTS_DIR/$APP_NAME"

rm -rf "$APP_DIR"
mkdir -p "$APP_DIR/Contents/MacOS"
mkdir -p "$APP_DIR/Contents/Resources"

# Copy binary
cp "$ROOT_DIR/target/release/ReShift" "$APP_DIR/Contents/MacOS/ReShift"
chmod +x "$APP_DIR/Contents/MacOS/ReShift"

# Copy icon
cp "$ROOT_DIR/crates/app/res/AppIcon.icns" "$APP_DIR/Contents/Resources/AppIcon.icns"

# Generate Info.plist
sed -e "s/{BUNDLE_ID}/$BUNDLE_ID/g" \
    -e "s/{VERSION}/$VERSION/g" \
    "$ROOT_DIR/crates/app/res/Info.plist.template" > "$APP_DIR/Contents/Info.plist"

# PkgInfo
printf "APPL????" > "$APP_DIR/Contents/PkgInfo"

echo "=== Codesigning ==="
IDENTITY=""
if [ "$ADHOC" = false ]; then
    # Look for Apple Development identity
    IDENTITY=$(security find-identity -v -p codesigning 2>/dev/null | grep "Apple Development" | head -1 | awk -F'"' '{print $2}' || true)
fi

if [ -n "$IDENTITY" ]; then
    echo "Signing with identity: $IDENTITY"
    codesign --force --options runtime --deep -s "$IDENTITY" "$APP_DIR"
else
    echo "Signing with ad-hoc identity (-)"
    codesign --force --deep -s - "$APP_DIR"
fi

echo ""
echo "=== Verification ==="
codesign -dvvv "$APP_DIR" 2>&1

echo ""
echo "=== Bundle Size ==="
du -sh "$APP_DIR"
ls -la "$APP_DIR/Contents/MacOS/ReShift" | awk '{print "Executable size:", $5, "bytes"}'
ls -la "$APP_DIR/Contents/Resources/AppIcon.icns" | awk '{print "Icon size:", $5, "bytes"}'

echo ""
echo "Created: $APP_DIR"
