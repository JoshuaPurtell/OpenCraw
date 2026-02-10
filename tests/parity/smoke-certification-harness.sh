#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/../.." && pwd)"
tmp_dir="$(mktemp -d "/tmp/opencraw-cert-smoke.XXXXXX")"

cleanup() {
    rm -rf "$tmp_dir"
}
trap cleanup EXIT

"${repo_root}/scripts/parity/collect-tier-evidence.sh" \
    --tier T1 \
    --tier T4 \
    --evidence-dir "$tmp_dir"

for tier in t1 t4; do
    report_path="$(find "$tmp_dir" -maxdepth 1 -type f -name "${tier}-*.md" | sort | tail -n 1)"
    if [[ -z "$report_path" ]]; then
        echo "missing evidence report for ${tier}" >&2
        exit 1
    fi
    rg -q "^## Check Results" "$report_path"
    rg -q "^- Overall Decision:" "$report_path"
    if [[ "$tier" == "t1" ]]; then
        rg -q "cargo test -p os-app --locked pairing::tests::unknown_sender_creates_pending_pairing_request -- --exact" "$report_path"
        rg -q "cargo test -p os-channels --locked telegram::tests::inbound_builders_handle_partial_payloads_without_panicking -- --exact" "$report_path"
    fi
done

echo "certification smoke harness passed"
