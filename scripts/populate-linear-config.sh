#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

CONFIG_ROOT="${OPENCRAW_CONFIG_ROOT:-$HOME/.opencraw}"
LINEAR_CONFIG_PATH="${OPENCRAW_LINEAR_CONFIG_PATH:-$CONFIG_ROOT/configs/channel-linear.toml}"
BACKUP_DIR="${OPENCRAW_BACKUP_DIR:-$CONFIG_ROOT/backups}"
LINEAR_GRAPHQL_URL="https://api.linear.app/graphql"

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

normalize_bool() {
  local raw="$1"
  local name="$2"
  local lowered
  lowered="$(echo "$raw" | tr '[:upper:]' '[:lower:]')"
  case "$lowered" in
    true|1|yes|y|on) echo "true" ;;
    false|0|no|n|off) echo "false" ;;
    *)
      echo "ERROR: $name must be a boolean (true/false), got: $raw" >&2
      exit 1
      ;;
  esac
}

normalize_access_mode() {
  local raw="$1"
  local lowered
  lowered="$(echo "$raw" | tr '[:upper:]' '[:lower:]')"
  case "$lowered" in
    pairing|allowlist|open) echo "$lowered" ;;
    *)
      echo "ERROR: OPENCRAW_LINEAR_ACCESS_MODE must be one of: pairing, allowlist, open" >&2
      exit 1
      ;;
  esac
}

normalize_positive_int() {
  local raw="$1"
  local name="$2"
  if [[ ! "$raw" =~ ^[0-9]+$ ]] || [[ "$raw" -le 0 ]]; then
    echo "ERROR: $name must be a positive integer, got: $raw" >&2
    exit 1
  fi
  echo "$raw"
}

trim_spaces() {
  local raw="$1"
  echo "$raw" | sed 's/^[[:space:]]*//;s/[[:space:]]*$//'
}

csv_to_toml_array() {
  local raw="$1"
  local trimmed
  trimmed="$(trim_spaces "$raw")"
  if [[ -z "$trimmed" ]]; then
    echo "[]"
    return
  fi

  local out=""
  local entry clean escaped
  IFS=',' read -r -a entries <<<"$raw"
  for entry in "${entries[@]}"; do
    clean="$(trim_spaces "$entry")"
    [[ -z "$clean" ]] && continue
    escaped="${clean//\\/\\\\}"
    escaped="${escaped//\"/\\\"}"
    if [[ -z "$out" ]]; then
      out="\"$escaped\""
    else
      out="$out, \"$escaped\""
    fi
  done
  echo "[${out}]"
}

fetch_teams_json() {
  local response
  if ! response="$(curl -sS \
    -X POST "$LINEAR_GRAPHQL_URL" \
    -H "Authorization: $OPENCRAW_LINEAR_API_KEY" \
    -H "Content-Type: application/json" \
    --data '{"query":"query Teams { teams(first: 50) { nodes { id key name } } }"}')"; then
    echo "ERROR: failed to reach Linear GraphQL API." >&2
    echo "Check network connectivity and OPENCRAW_LINEAR_API_KEY." >&2
    exit 1
  fi

  if echo "$response" | tr -d '\n' | grep -q '"errors"[[:space:]]*:'; then
    echo "ERROR: failed to query Linear teams. Check OPENCRAW_LINEAR_API_KEY." >&2
    echo "Response:" >&2
    echo "$response" >&2
    exit 1
  fi

  echo "$response"
}

extract_teams_tsv() {
  local json="$1"
  echo "$json" | tr -d '\n' | sed 's/},{/}\n{/g' \
    | sed -n 's/.*"id":"\([^"]*\)","key":"\([^"]*\)","name":"\([^"]*\)".*/\1\t\2\t\3/p'
}

team_exists_in_list() {
  local team_id="$1"
  local teams_tsv="$2"
  echo "$teams_tsv" | awk -F '\t' -v target="$team_id" '$1 == target { found = 1 } END { exit(found ? 0 : 1) }'
}

resolve_team_id_from_selector() {
  local selector="$1"
  local teams_tsv="$2"

  # Exact id/key/name match first (case-insensitive).
  local matches
  matches="$(echo "$teams_tsv" | awk -F '\t' -v s="$selector" '
BEGIN {
  sl = tolower(s);
  count = 0;
}
NF > 0 {
  id = tolower($1);
  key = tolower($2);
  name = tolower($3);
  if (id == sl || key == sl || name == sl) {
    count += 1;
    ids[count] = $1;
    keys[count] = $2;
    names[count] = $3;
  }
}
END {
  if (count == 1) {
    printf "ok\t%s\t%s\t%s\n", ids[1], keys[1], names[1];
    exit 0;
  }
  if (count > 1) {
    printf "ambiguous_exact\t%d\n", count;
    for (i = 1; i <= count; i++) {
      printf "%s\t%s\t%s\n", ids[i], keys[i], names[i];
    }
    exit 0;
  }
  print "none";
}
')"

  local header
  header="$(echo "$matches" | head -n1)"
  if [[ "$header" == ok$'\t'* ]]; then
    echo "$matches"
    return
  fi
  if [[ "$header" == ambiguous_exact$'\t'* ]]; then
    echo "$matches"
    return
  fi

  # Fallback: contains match on key/name (case-insensitive).
  matches="$(echo "$teams_tsv" | awk -F '\t' -v s="$selector" '
BEGIN {
  sl = tolower(s);
  count = 0;
}
NF > 0 {
  key = tolower($2);
  name = tolower($3);
  if (index(key, sl) > 0 || index(name, sl) > 0) {
    count += 1;
    ids[count] = $1;
    keys[count] = $2;
    names[count] = $3;
  }
}
END {
  if (count == 1) {
    printf "ok\t%s\t%s\t%s\n", ids[1], keys[1], names[1];
    exit 0;
  }
  if (count > 1) {
    printf "ambiguous_contains\t%d\n", count;
    for (i = 1; i <= count; i++) {
      printf "%s\t%s\t%s\n", ids[i], keys[i], names[i];
    }
    exit 0;
  }
  print "none";
}
')"

  echo "$matches"
}

resolve_default_team_id() {
  local configured_team_id="$1"
  local configured_selector="$2"
  local teams_tsv="$3"

  if [[ -n "$configured_team_id" ]]; then
    if ! team_exists_in_list "$configured_team_id" "$teams_tsv"; then
      echo "WARN: OPENCRAW_LINEAR_DEFAULT_TEAM_ID is not in the first 50 visible teams for this key." >&2
      echo "      Continuing anyway with the configured value." >&2
    fi
    echo "$configured_team_id"
    return
  fi

  if [[ -n "$configured_selector" ]]; then
    local lookup first_line team_id team_key team_name
    lookup="$(resolve_team_id_from_selector "$configured_selector" "$teams_tsv")"
    first_line="$(echo "$lookup" | head -n1)"
    if [[ "$first_line" == ok$'\t'* ]]; then
      team_id="$(echo "$first_line" | cut -f2)"
      team_key="$(echo "$first_line" | cut -f3)"
      team_name="$(echo "$first_line" | cut -f4)"
      echo "Using Linear team: id=$team_id key=$team_key name=$team_name" >&2
      echo "$team_id"
      return
    fi

    if [[ "$first_line" == ambiguous_* ]]; then
      echo "ERROR: OPENCRAW_LINEAR_TEAM matched multiple teams; be more specific." >&2
      echo "Matches:" >&2
      echo "$lookup" | tail -n +2 | awk -F '\t' 'NF > 0 { printf "  - id=%s key=%s name=%s\n", $1, $2, $3 }' >&2
      exit 1
    fi

    echo "ERROR: OPENCRAW_LINEAR_TEAM did not match any visible team." >&2
    echo "Selector: $configured_selector" >&2
    echo "Visible teams:" >&2
    echo "$teams_tsv" | awk -F '\t' 'NF > 0 { printf "  - id=%s key=%s name=%s\n", $1, $2, $3 }' >&2
    exit 1
  fi

  local team_count
  team_count="$(echo "$teams_tsv" | awk 'NF > 0 { count += 1 } END { print count + 0 }')"

  if [[ "$team_count" -eq 0 ]]; then
    echo "ERROR: no visible Linear teams found for this API key." >&2
    echo "Set OPENCRAW_LINEAR_DEFAULT_TEAM_ID explicitly and retry." >&2
    exit 1
  fi

  if [[ "$team_count" -eq 1 ]]; then
    local only_team_id
    only_team_id="$(echo "$teams_tsv" | awk -F '\t' 'NF > 0 { print $1; exit }')"
    echo "$only_team_id"
    return
  fi

  echo "ERROR: multiple Linear teams found. Set OPENCRAW_LINEAR_TEAM or OPENCRAW_LINEAR_DEFAULT_TEAM_ID and retry." >&2
  echo "Visible teams:" >&2
  echo "$teams_tsv" | awk -F '\t' 'NF > 0 { printf "  - id=%s key=%s name=%s\n", $1, $2, $3 }' >&2
  exit 1
}

write_linear_config() {
  local enabled="$1"
  local api_key="$2"
  local default_team_id="$3"
  local poll_interval_ms="$4"
  local team_ids_toml="$5"
  local start_from_latest="$6"
  local action_whoami="$7"
  local action_list_assigned="$8"
  local action_list_users="$9"
  local action_list_teams="${10}"
  local action_list_projects="${11}"
  local action_create_issue="${12}"
  local action_create_project="${13}"
  local action_update_issue="${14}"
  local action_assign_issue="${15}"
  local action_comment_issue="${16}"
  local access_mode="${17}"
  local allowed_senders_toml="${18}"
  local path="${19}"

  local timestamp backup
  mkdir -p "$(dirname "$path")"
  mkdir -p "$BACKUP_DIR"

  backup=""
  if [[ -f "$path" ]]; then
    timestamp="$(date -u '+%Y%m%dT%H%M%SZ')"
    backup="$BACKUP_DIR/$(basename "$path").bak.${timestamp}"
    cp "$path" "$backup"
  fi

  cat >"$path" <<EOF
[channels.linear]
enabled = $enabled
api_key = "$api_key"
default_team_id = "$default_team_id"
poll_interval_ms = $poll_interval_ms
team_ids = $team_ids_toml
start_from_latest = $start_from_latest

[channels.linear.actions]
whoami = $action_whoami
list_assigned = $action_list_assigned
list_users = $action_list_users
list_teams = $action_list_teams
list_projects = $action_list_projects
create_issue = $action_create_issue
create_project = $action_create_project
update_issue = $action_update_issue
assign_issue = $action_assign_issue
comment_issue = $action_comment_issue

[channels.linear.access]
mode = "$access_mode"
allowed_senders = $allowed_senders_toml
EOF

  chmod 600 "$path"
  echo "$backup"
}

main() {
  load_env
  require_cmd curl
  require_env OPENCRAW_LINEAR_API_KEY

  local enabled start_from_latest poll_interval_ms access_mode
  local action_whoami action_list_assigned action_list_users action_list_teams action_list_projects
  local action_create_issue action_create_project action_update_issue action_assign_issue action_comment_issue
  enabled="$(normalize_bool "${OPENCRAW_LINEAR_ENABLED:-true}" "OPENCRAW_LINEAR_ENABLED")"
  start_from_latest="$(normalize_bool "${OPENCRAW_LINEAR_START_FROM_LATEST:-true}" "OPENCRAW_LINEAR_START_FROM_LATEST")"
  poll_interval_ms="$(normalize_positive_int "${OPENCRAW_LINEAR_POLL_INTERVAL_MS:-3000}" "OPENCRAW_LINEAR_POLL_INTERVAL_MS")"
  access_mode="$(normalize_access_mode "${OPENCRAW_LINEAR_ACCESS_MODE:-pairing}")"
  action_whoami="$(normalize_bool "${OPENCRAW_LINEAR_ACTION_WHOAMI:-true}" "OPENCRAW_LINEAR_ACTION_WHOAMI")"
  action_list_assigned="$(normalize_bool "${OPENCRAW_LINEAR_ACTION_LIST_ASSIGNED:-true}" "OPENCRAW_LINEAR_ACTION_LIST_ASSIGNED")"
  action_list_users="$(normalize_bool "${OPENCRAW_LINEAR_ACTION_LIST_USERS:-true}" "OPENCRAW_LINEAR_ACTION_LIST_USERS")"
  action_list_teams="$(normalize_bool "${OPENCRAW_LINEAR_ACTION_LIST_TEAMS:-true}" "OPENCRAW_LINEAR_ACTION_LIST_TEAMS")"
  action_list_projects="$(normalize_bool "${OPENCRAW_LINEAR_ACTION_LIST_PROJECTS:-true}" "OPENCRAW_LINEAR_ACTION_LIST_PROJECTS")"
  action_create_issue="$(normalize_bool "${OPENCRAW_LINEAR_ACTION_CREATE_ISSUE:-true}" "OPENCRAW_LINEAR_ACTION_CREATE_ISSUE")"
  action_create_project="$(normalize_bool "${OPENCRAW_LINEAR_ACTION_CREATE_PROJECT:-true}" "OPENCRAW_LINEAR_ACTION_CREATE_PROJECT")"
  action_update_issue="$(normalize_bool "${OPENCRAW_LINEAR_ACTION_UPDATE_ISSUE:-true}" "OPENCRAW_LINEAR_ACTION_UPDATE_ISSUE")"
  action_assign_issue="$(normalize_bool "${OPENCRAW_LINEAR_ACTION_ASSIGN_ISSUE:-true}" "OPENCRAW_LINEAR_ACTION_ASSIGN_ISSUE")"
  action_comment_issue="$(normalize_bool "${OPENCRAW_LINEAR_ACTION_COMMENT_ISSUE:-true}" "OPENCRAW_LINEAR_ACTION_COMMENT_ISSUE")"

  local teams_json teams_tsv default_team_id team_ids_toml allowed_senders_toml backup
  teams_json="$(fetch_teams_json)"
  teams_tsv="$(extract_teams_tsv "$teams_json")"
  default_team_id="$(resolve_default_team_id "${OPENCRAW_LINEAR_DEFAULT_TEAM_ID:-}" "${OPENCRAW_LINEAR_TEAM:-}" "$teams_tsv")"
  team_ids_toml="$(csv_to_toml_array "${OPENCRAW_LINEAR_TEAM_IDS:-}")"
  allowed_senders_toml="$(csv_to_toml_array "${OPENCRAW_LINEAR_ALLOWED_SENDERS:-}")"

  backup="$(write_linear_config \
    "$enabled" \
    "$OPENCRAW_LINEAR_API_KEY" \
    "$default_team_id" \
    "$poll_interval_ms" \
    "$team_ids_toml" \
    "$start_from_latest" \
    "$action_whoami" \
    "$action_list_assigned" \
    "$action_list_users" \
    "$action_list_teams" \
    "$action_list_projects" \
    "$action_create_issue" \
    "$action_create_project" \
    "$action_update_issue" \
    "$action_assign_issue" \
    "$action_comment_issue" \
    "$access_mode" \
    "$allowed_senders_toml" \
    "$LINEAR_CONFIG_PATH")"

  echo "Populated Linear config."
  echo "Updated: $LINEAR_CONFIG_PATH"
  if [[ -n "$backup" ]]; then
    echo "Backup:  $backup"
  fi
  echo "default_team_id: $default_team_id"
}

main "$@"
