#!/usr/bin/env bash
set -euo pipefail

usage() {
    cat <<'EOF'
Usage: scripts/parity/check-tier-certification.sh --tier T1|T2|T3|T4 [options]

Options:
  --tier <T1|T2|T3|T4>  Tier to evaluate (required)
  --output <path>        Output evidence markdown path
  --allow-fail           Always exit 0 even when checks fail
  --include-cargo        Include `cargo check --workspace --all-targets --locked`
  -h, --help             Show help
EOF
}

tier=""
output=""
allow_fail=0
include_cargo=0

while [[ $# -gt 0 ]]; do
    case "$1" in
        --tier)
            tier="${2:-}"
            shift 2
            ;;
        --output)
            output="${2:-}"
            shift 2
            ;;
        --allow-fail)
            allow_fail=1
            shift
            ;;
        --include-cargo)
            include_cargo=1
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            echo "unknown argument: $1" >&2
            usage >&2
            exit 2
            ;;
    esac
done

if [[ -z "$tier" ]]; then
    echo "--tier is required" >&2
    usage >&2
    exit 2
fi

case "$tier" in
    T1|T2|T3|T4) ;;
    *)
        echo "invalid tier: $tier (expected one of T1, T2, T3, T4)" >&2
        exit 2
        ;;
esac

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/../.." && pwd)"
tier_lower="$(echo "$tier" | tr '[:upper:]' '[:lower:]')"
timestamp="$(date -u +%Y-%m-%dT%H:%M:%SZ)"

if [[ -z "$output" ]]; then
    output="${repo_root}/plans/parallel-agents/certification/evidence/${tier_lower}-$(date -u +%Y%m%dT%H%M%SZ).md"
fi
mkdir -p "$(dirname "$output")"

pass_count=0
fail_count=0
skip_count=0
rows=()
failed_checks=()

file_has() {
    local rel="$1"
    local needle="$2"
    rg -q --fixed-strings "$needle" "${repo_root}/${rel}"
}

file_has_re() {
    local rel="$1"
    local pattern="$2"
    rg -q "$pattern" "${repo_root}/${rel}"
}

file_has_all() {
    local rel="$1"
    shift
    local needle
    for needle in "$@"; do
        file_has "$rel" "$needle" || return 1
    done
}

all_executable() {
    local rel
    for rel in "$@"; do
        [[ -x "${repo_root}/${rel}" ]] || return 1
    done
}

cargo_check_gate() {
    (
        cd "$repo_root"
        cargo check --workspace --all-targets --locked >/dev/null
    )
}

tier_t1_channels_registered() {
    file_has "os-app/src/channel_plugins.rs" 'Self::Telegram => "telegram"'
}

tier_t1_delivery_controls_present() {
    file_has "os-channels/src/telegram.rs" "offset = update.update_id + 1;"
}

tier_t1_session_routing_present() {
    file_has_all "os-channels/src/telegram.rs" \
        'let is_group = m.chat.r#type != "private";' \
        "thread_id: Some(m.chat.id.to_string().into()),"
}

tier_t2_channels_registered() {
    file_has "os-app/src/channel_plugins.rs" 'Self::Email => "email"'
}

tier_t2_delivery_controls_present() {
    file_has_all "os-channels/src/email.rs" \
        "let mut seen_message_ids = HashSet::<String>::new();" \
        "if seen_message_ids.contains(&message.message_id) {"
}

tier_t2_session_routing_present() {
    file_has_all "os-channels/src/email.rs" \
        "thread_id: Some(message.thread_id.clone().into())," \
        "is_group: false,"
}

tier_t3_channels_registered() {
    file_has_all "os-app/src/channel_plugins.rs" \
        'Self::Discord => "discord"' \
        'Self::Slack => "slack"' \
        'Self::Matrix => "matrix"' \
        'Self::Signal => "signal"' \
        'Self::Whatsapp => "whatsapp"'
}

tier_t3_delivery_controls_present() {
    file_has "os-channels/src/discord.rs" "let seq: Arc<RwLock<Option<i64>>> = Arc::new(RwLock::new(None));" &&
        file_has "os-channels/src/slack.rs" "let mut cursor_by_channel: HashMap<String, String> = HashMap::new();" &&
        file_has "os-channels/src/matrix.rs" "let mut since_token: Option<String> = None;" &&
        file_has "os-channels/src/signal.rs" "let mut cursor_millis: Option<i64> = None;" &&
        file_has "os-app/src/channel_plugins.rs" "fn verify_whatsapp_signature(headers: &HeaderMap, body: &[u8], app_secret: &str) -> bool"
}

tier_t3_session_routing_present() {
    file_has "os-channels/src/discord.rs" "let is_group = event.guild_id.is_some();" &&
        file_has "os-channels/src/discord.rs" "thread_id: Some(event.channel_id.into())," &&
        file_has "os-channels/src/slack.rs" "thread_id: Some(" &&
        file_has "os-channels/src/matrix.rs" "thread_id: Some(room_id.clone().into())," &&
        file_has "os-channels/src/signal.rs" "let is_group = group_id.is_some();" &&
        file_has "os-app/src/channel_plugins.rs" 'let thread_id = format!("wa:{}:{}", phone_number_id, sender);'
}

tier_t4_channels_registered() {
    file_has "os-app/src/config.rs" "pub external_plugins: Vec<ExternalChannelPluginConfig>," &&
        file_has "os-app/src/channel_plugins.rs" ".external_plugins" &&
        file_has "os-app/src/channel_plugins.rs" "HttpPluginAdapter::new(&channel_id, &plugin_cfg.send_url)?"
}

tier_t4_delivery_controls_present() {
    file_has_all "os-channels/src/http_plugin.rs" \
        "let mut recent_event_id_set = HashSet::<String>::new();" \
        "if recent_event_id_set.contains(&normalized.event_id) {" \
        "remember_event_id("
}

tier_t4_session_routing_present() {
    file_has_all "os-channels/src/http_plugin.rs" \
        "thread_id: self.thread_id.map(Into::into)," \
        "is_group: self.is_group,"
}

run_check() {
    local check_id="$1"
    local criterion="$2"
    local description="$3"
    local evidence_cmd="$4"
    shift 4

    local result="FAIL"
    if "$@"; then
        result="PASS"
        pass_count=$((pass_count + 1))
    else
        fail_count=$((fail_count + 1))
        failed_checks+=("${check_id}: ${description}")
    fi

    rows+=("| ${check_id} | ${criterion} | ${description} | ${result} | \`${evidence_cmd}\` |")
}

run_skip() {
    local check_id="$1"
    local criterion="$2"
    local description="$3"
    local reason="$4"
    skip_count=$((skip_count + 1))
    rows+=("| ${check_id} | ${criterion} | ${description} | SKIP | \`${reason}\` |")
}

run_check "C1.1" "C1" "default deny for external channels is present" \
    "rg -q external_channels_denied_by_default os-app/src/pairing.rs" \
    file_has "os-app/src/pairing.rs" "external_channels_denied_by_default"

run_check "C1.2" "C1" "allowlist requirement text is present" \
    "rg -q 'require explicit allowlisting' os-app/src/pairing.rs" \
    file_has "os-app/src/pairing.rs" "require explicit allowlisting"

run_check "C2.1" "C2" "automation ingest dedupe counters are tracked" \
    "rg -q webhook_duplicate_events os-app/src/automation_runtime.rs" \
    file_has "os-app/src/automation_runtime.rs" "webhook_duplicate_events"

run_check "C2.2" "C2" "automation ingest event-id uniqueness index exists" \
    "rg -q 'ON opencraw_automation_ingest_events (ingest_kind, source, event_id)' os-app/src/automation_runtime.rs" \
    file_has "os-app/src/automation_runtime.rs" "ON opencraw_automation_ingest_events (ingest_kind, source, event_id)"

run_check "C3.1" "C3" "session storage key enforces channel_id + sender_id isolation" \
    "rg -q 'PRIMARY KEY (channel_id, sender_id)' os-app/src/session.rs" \
    file_has "os-app/src/session.rs" "PRIMARY KEY (channel_id, sender_id)"

run_check "C4.1" "C4" "health endpoint exists" \
    "rg -q '/api/v1/os/health' os-app/src/routes/health.rs" \
    file_has "os-app/src/routes/health.rs" "/api/v1/os/health"

run_check "C4.2" "C4" "channel diagnostics list endpoint exists" \
    "rg -q '/api/v1/os/channels' os-app/src/routes/channels.rs" \
    file_has "os-app/src/routes/channels.rs" "/api/v1/os/channels"

run_check "C4.3" "C4" "automation status endpoint exists" \
    "rg -q '/api/v1/os/automation/status' os-app/src/routes/automation.rs" \
    file_has "os-app/src/routes/automation.rs" "/api/v1/os/automation/status"

run_check "C4.4" "C4" "doctor CLI command exists" \
    "rg -q 'Doctor {' os-app/src/main.rs" \
    file_has "os-app/src/main.rs" "Doctor {"

run_check "C4.5" "C4" "status CLI command exists" \
    "rg -q '^[[:space:]]*Status[[:space:]]*\\{' os-app/src/main.rs" \
    file_has_re "os-app/src/main.rs" "^[[:space:]]*Status[[:space:]]*\\{"

run_check "C4.6" "C4" "channel-specific probe endpoint exists" \
    "rg -q '/api/v1/os/channels/{channel_id}/probe' os-app/src/routes/channels.rs" \
    file_has "os-app/src/routes/channels.rs" "/api/v1/os/channels/{channel_id}/probe"

case "$tier" in
    T1)
        run_check "T1-C1.3" "C1" "telegram channel is registered in plugin matrix" \
            "tier_t1_channels_registered" tier_t1_channels_registered
        run_check "T1-C2.3" "C2" "telegram poll offset progression exists" \
            "tier_t1_delivery_controls_present" tier_t1_delivery_controls_present
        run_check "T1-C3.2" "C3" "telegram thread + group routing fields exist" \
            "tier_t1_session_routing_present" tier_t1_session_routing_present
        ;;
    T2)
        run_check "T2-C1.3" "C1" "email channel is registered in plugin matrix" \
            "tier_t2_channels_registered" tier_t2_channels_registered
        run_check "T2-C2.3" "C2" "email dedupe controls exist" \
            "tier_t2_delivery_controls_present" tier_t2_delivery_controls_present
        run_check "T2-C3.2" "C3" "email thread routing fields exist" \
            "tier_t2_session_routing_present" tier_t2_session_routing_present
        ;;
    T3)
        run_check "T3-C1.3" "C1" "all T3 channels are registered in plugin matrix" \
            "tier_t3_channels_registered" tier_t3_channels_registered
        run_check "T3-C2.3" "C2" "all T3 channels expose dedupe/cursor controls" \
            "tier_t3_delivery_controls_present" tier_t3_delivery_controls_present
        run_check "T3-C3.2" "C3" "all T3 channels expose routing invariants" \
            "tier_t3_session_routing_present" tier_t3_session_routing_present
        ;;
    T4)
        run_check "T4-C1.3" "C1" "external plugin channels are configured and wired" \
            "tier_t4_channels_registered" tier_t4_channels_registered
        run_check "T4-C2.3" "C2" "external plugin dedupe/event replay controls exist" \
            "tier_t4_delivery_controls_present" tier_t4_delivery_controls_present
        run_check "T4-C3.2" "C3" "external plugin thread/group routing fields exist" \
            "tier_t4_session_routing_present" tier_t4_session_routing_present
        ;;
esac

run_check "C5.1" "C5" "certification scripts and smoke harness are executable" \
    "test -x scripts/parity/check-tier-certification.sh && test -x scripts/parity/collect-tier-evidence.sh && test -x tests/parity/smoke-certification-harness.sh" \
    all_executable \
    "scripts/parity/check-tier-certification.sh" \
    "scripts/parity/collect-tier-evidence.sh" \
    "tests/parity/smoke-certification-harness.sh"

if [[ "$include_cargo" -eq 1 ]]; then
    run_check "C5.2" "C5" "workspace cargo check gate passes" \
        "cargo check --workspace --all-targets --locked" \
        cargo_check_gate
else
    run_skip "C5.2" "C5" "workspace cargo check gate passes" "cargo gate skipped (use --include-cargo)"
fi

overall="PASS"
if (( fail_count > 0 )); then
    overall="FAIL"
elif (( skip_count > 0 )); then
    overall="CONDITIONAL"
fi

{
    echo "# ${tier} Certification Evidence"
    echo
    echo "- Generated (UTC): ${timestamp}"
    echo "- Tier: ${tier}"
    echo "- Repository: ${repo_root}"
    echo "- Criteria Source: plans/parallel-agents/certification/tier-criteria.md"
    echo
    echo "## Exact Tier Criteria"
    echo
    echo "1. Auth, pairing, and allowlist/approval boundaries are enforced by default."
    echo "2. Delivery is deterministic and replay-safe (idempotent ingest, dedupe, bounded retries/backoff)."
    echo "3. Session routing invariants hold (DM vs group isolation, mention/reply behavior, ordering guarantees)."
    echo "4. Operator diagnostics are complete (\`status\`, logs, health, channel-specific probes)."
    echo "5. Tier acceptance tests pass and stay green in CI."
    echo
    echo "## Check Results"
    echo
    echo "| Check ID | Criterion | Description | Result | Evidence Command |"
    echo "|---|---|---|---|---|"
    printf '%s\n' "${rows[@]}"
    echo
    echo "## Summary"
    echo
    echo "- Pass: ${pass_count}"
    echo "- Fail: ${fail_count}"
    echo "- Skip: ${skip_count}"
    echo "- Overall Decision: ${overall}"
} >"$output"

if (( ${#failed_checks[@]} > 0 )); then
    {
        echo
        echo "## Failed Checks"
        echo
        printf -- "- %s\n" "${failed_checks[@]}"
    } >>"$output"
fi

echo "$output"

if (( fail_count > 0 )) && (( allow_fail == 0 )); then
    exit 1
fi

