#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

load_env() {
  [[ -f .env ]] && { set -a; source .env; set +a; }
}

preflight() {
  if [[ -z "${ANTHROPIC_API_KEY:-}" && -z "${OPENAI_API_KEY:-}" ]]; then
    echo "WARNING: No LLM API key set (ANTHROPIC_API_KEY or OPENAI_API_KEY)." >&2
  fi
  if [[ ! -f "$HOME/.opencraw/config.toml" ]]; then
    echo "WARNING: ~/.opencraw/config.toml missing — cp config.example.toml ~/.opencraw/config.toml" >&2
  fi
}

main() {
  load_env
  preflight
  export OPENCRAW_LOG_FORMAT="${OPENCRAW_LOG_FORMAT:-pretty}"
  cargo run -p os-app -- serve "$@"
}

main "$@"
