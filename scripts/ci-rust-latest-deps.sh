#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

echo "==> cargo update"
cargo update

echo "==> cargo check (latest deps)"
cargo check --workspace --all-targets

echo "==> cargo clippy (latest deps)"
cargo clippy --workspace --all-targets --all-features -- -D warnings

echo "==> cargo test (latest deps)"
cargo test --workspace --all-targets
