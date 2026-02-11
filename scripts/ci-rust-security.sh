#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

echo "==> cargo audit"
cargo audit

echo "==> cargo deny"
cargo deny --log-level warn check advisories bans licenses sources
