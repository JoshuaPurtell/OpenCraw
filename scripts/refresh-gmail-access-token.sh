#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

CONFIG_ROOT="${OPENCRAW_CONFIG_ROOT:-$HOME/.opencraw}"
EMAIL_CONFIG_PATH="${OPENCRAW_EMAIL_CONFIG_PATH:-$CONFIG_ROOT/configs/channel-email.toml}"
BACKUP_DIR="${OPENCRAW_BACKUP_DIR:-$CONFIG_ROOT/backups}"

load_env() {
  [[ -f .env ]] && { set -a; source .env; set +a; }
}

require_cmd() {
  local cmd="$1"
  if ! command -v "$cmd" >/dev/null 2>&1; then
    echo "ERROR: required command not found: $cmd" >&2
    exit 1
  fi
}

require_env() {
  local name="$1"
  if [[ -z "${!name:-}" ]]; then
    echo "ERROR: missing required env var: $name" >&2
    exit 1
  fi
}

extract_json_string() {
  local key="$1"
  local json="$2"
  echo "$json" | tr -d '\n' | sed -n "s/.*\"$key\"[[:space:]]*:[[:space:]]*\"\\([^\"]*\\)\".*/\\1/p"
}

extract_json_number() {
  local key="$1"
  local json="$2"
  echo "$json" | tr -d '\n' | sed -n "s/.*\"$key\"[[:space:]]*:[[:space:]]*\\([0-9][0-9]*\\).*/\\1/p"
}

refresh_token() {
  local response
  response="$(curl -sS \
    -X POST "https://oauth2.googleapis.com/token" \
    -H "Content-Type: application/x-www-form-urlencoded" \
    --data-urlencode "client_id=$OPENCRAW_GMAIL_OAUTH_CLIENT_ID" \
    --data-urlencode "client_secret=$OPENCRAW_GMAIL_OAUTH_CLIENT_SECRET" \
    --data-urlencode "refresh_token=$OPENCRAW_GMAIL_OAUTH_REFRESH_TOKEN" \
    --data-urlencode "grant_type=refresh_token")"

  local access_token
  access_token="$(extract_json_string "access_token" "$response")"
  if [[ -z "$access_token" ]]; then
    echo "ERROR: failed to refresh Gmail access token." >&2
    echo "Response:" >&2
    echo "$response" >&2
    exit 1
  fi

  local expires_in
  expires_in="$(extract_json_number "expires_in" "$response")"
  if [[ -z "$expires_in" ]]; then
    expires_in="unknown"
  fi

  echo "$access_token|$expires_in"
}

write_token_to_email_config() {
  local token="$1"
  local file="$2"
  local timestamp backup tmp file_base

  if [[ ! -f "$file" ]]; then
    echo "ERROR: email config file not found: $file" >&2
    exit 1
  fi

  mkdir -p "$BACKUP_DIR"
  timestamp="$(date -u '+%Y%m%dT%H%M%SZ')"
  file_base="$(basename "$file")"
  backup="$BACKUP_DIR/${file_base}.bak.${timestamp}"
  cp "$file" "$backup"

  tmp="$(mktemp)"
  awk -v token="$token" '
BEGIN {
  in_email = 0
  updated = 0
}
{
  if ($0 ~ /^\[channels\.email\][[:space:]]*$/) {
    in_email = 1
    print
    next
  }

  if ($0 ~ /^\[[^]]+\][[:space:]]*$/ && $0 !~ /^\[channels\.email\][[:space:]]*$/) {
    if (in_email == 1 && updated == 0) {
      print "gmail_access_token = \"" token "\""
      updated = 1
    }
    in_email = 0
  }

  if (in_email == 1 && $0 ~ /^[[:space:]]*#?[[:space:]]*gmail_access_token[[:space:]]*=/) {
    if (updated == 0) {
      print "gmail_access_token = \"" token "\""
      updated = 1
    }
    next
  }

  print
}
END {
  if (in_email == 1 && updated == 0) {
    print "gmail_access_token = \"" token "\""
    updated = 1
  }
  if (updated == 0) {
    print ""
    print "[channels.email]"
    print "gmail_access_token = \"" token "\""
  }
}
' "$file" >"$tmp"

  mv "$tmp" "$file"
  echo "$backup"
}

main() {
  load_env
  require_cmd curl
  require_env OPENCRAW_GMAIL_OAUTH_CLIENT_ID
  require_env OPENCRAW_GMAIL_OAUTH_CLIENT_SECRET
  require_env OPENCRAW_GMAIL_OAUTH_REFRESH_TOKEN

  local refresh_result access_token expires_in backup
  refresh_result="$(refresh_token)"
  access_token="${refresh_result%%|*}"
  expires_in="${refresh_result#*|}"
  backup="$(write_token_to_email_config "$access_token" "$EMAIL_CONFIG_PATH")"

  echo "Refreshed Gmail access token."
  echo "Updated: $EMAIL_CONFIG_PATH"
  echo "Backup:  $backup"
  echo "expires_in_seconds: $expires_in"
}

main "$@"
