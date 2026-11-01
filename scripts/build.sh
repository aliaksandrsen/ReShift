#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

echo "=== 1. Checking format (cargo fmt) ==="
cargo fmt --all -- --check

echo "=== 2. Running clippy ==="
cargo clippy --workspace --all-targets -- -D warnings

echo "=== 3. Running unit tests ==="
cargo test --workspace

echo "=== 4. Packaging .app bundle ==="
"$ROOT_DIR/scripts/bundle.sh" "$@"
