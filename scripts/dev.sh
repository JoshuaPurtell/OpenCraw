#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"
CONFIG_ROOT="${OPENCRAW_CONFIG_ROOT:-$HOME/.opencraw}"
CONFIG_FILE="$CONFIG_ROOT/config.toml"
KEYS_FILE="$CONFIG_ROOT/configs/keys.toml"
LOG_DIR="${OPENCRAW_LOG_DIR:-$CONFIG_ROOT/logs}"
LOG_FILE=""

ts_now() {
  date -u "+%Y-%m-%dT%H:%M:%SZ"
}

log_line() {
  local message="$*"
  local line
  line="$(ts_now) ${message}"
  echo "$line"
  if [[ -n "$LOG_FILE" ]]; then
    echo "$line" >>"$LOG_FILE"
  fi
}

timestamp_stream() {
  while IFS= read -r line || [[ -n "$line" ]]; do
    log_line "$line"
  done
}

run_with_timestamps() {
  "$@" 2>&1 | timestamp_stream
}

load_env() {
  [[ -f .env ]] && { set -a; source .env; set +a; }
}

require_cmd() {
  local cmd="$1"
  if ! command -v "$cmd" >/dev/null 2>&1; then
    log_line "ERROR: required command not found: $cmd"
    exit 1
  fi
}

ensure_local_config() {
  if [[ -f "$CONFIG_FILE" && -f "$KEYS_FILE" ]]; then
    return 0
  fi

  log_line "Initializing local config under $CONFIG_ROOT ..."
  run_with_timestamps cargo run -p os-app -- init
}

warn_if_llm_key_missing() {
  if [[ ! -f "$KEYS_FILE" ]]; then
    log_line "WARNING: missing $KEYS_FILE"
    return 0
  fi

  if ! grep -Eq '^[[:space:]]*(openai_api_key|anthropic_api_key)[[:space:]]*=[[:space:]]*".+"' "$KEYS_FILE"; then
    log_line "WARNING: no LLM key found in $KEYS_FILE"
    log_line "Set keys.openai_api_key or keys.anthropic_api_key before chatting."
  fi
}

run_doctor() {
  if [[ "${OPENCRAW_SKIP_DOCTOR:-0}" == "1" ]]; then
    return 0
  fi

  log_line "Running config checks (opencraw doctor) ..."
  run_with_timestamps cargo run -p os-app -- doctor
}

main() {
  mkdir -p "$LOG_DIR"
  LOG_FILE="$LOG_DIR/dev-$(date -u '+%Y%m%dT%H%M%SZ').log"
  log_line "OpenCraw dev logs: $LOG_FILE"
  require_cmd cargo
  load_env
  ensure_local_config
  warn_if_llm_key_missing
  run_doctor
  export OPENCRAW_LOG_FORMAT="${OPENCRAW_LOG_FORMAT:-pretty}"
  log_line "Starting opencraw serve ..."
  run_with_timestamps cargo run -p os-app -- serve "$@"
}

main "$@"
