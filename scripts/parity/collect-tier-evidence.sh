#!/usr/bin/env bash
set -euo pipefail

usage() {
    cat <<'EOF'
Usage: scripts/parity/collect-tier-evidence.sh [options]

Options:
  --tier <T1|T2|T3|T4>   Tier to collect (repeatable). Defaults to all tiers.
  --evidence-dir <path>  Output directory for evidence files
  --include-cargo        Include cargo gate in each tier check
  --strict               Exit non-zero if any tier has failing checks
  -h, --help             Show help
EOF
}

tiers=()
evidence_dir=""
include_cargo=0
strict=0

while [[ $# -gt 0 ]]; do
    case "$1" in
        --tier)
            tiers+=("${2:-}")
            shift 2
            ;;
        --evidence-dir)
            evidence_dir="${2:-}"
            shift 2
            ;;
        --include-cargo)
            include_cargo=1
            shift
            ;;
        --strict)
            strict=1
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

if (( ${#tiers[@]} == 0 )); then
    tiers=(T1 T2 T3 T4)
fi

for tier in "${tiers[@]}"; do
    case "$tier" in
        T1|T2|T3|T4) ;;
        *)
            echo "invalid tier: $tier (expected one of T1, T2, T3, T4)" >&2
            exit 2
            ;;
    esac
done

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/../.." && pwd)"
check_script="${script_dir}/check-tier-certification.sh"

if [[ -z "$evidence_dir" ]]; then
    evidence_dir="${repo_root}/plans/parallel-agents/certification/evidence"
fi
mkdir -p "$evidence_dir"

generated=()
failed=()

for tier in "${tiers[@]}"; do
    tier_lower="$(echo "$tier" | tr '[:upper:]' '[:lower:]')"
    output_path="${evidence_dir}/${tier_lower}-$(date -u +%Y%m%dT%H%M%SZ).md"

    cmd=("$check_script" --tier "$tier" --output "$output_path")
    if [[ "$include_cargo" -eq 1 ]]; then
        cmd+=(--include-cargo)
    fi
    if [[ "$strict" -eq 0 ]]; then
        cmd+=(--allow-fail)
    fi

    if ! "${cmd[@]}"; then
        failed+=("$tier")
    fi

    if rg -q "^- Overall Decision: FAIL" "$output_path"; then
        failed+=("$tier")
    fi

    generated+=("$output_path")
    sleep 1
done

unique_failed=()
for tier in "${failed[@]-}"; do
    seen=0
    for existing in "${unique_failed[@]-}"; do
        if [[ "$existing" == "$tier" ]]; then
            seen=1
            break
        fi
    done
    if [[ "$seen" -eq 0 ]]; then
        unique_failed+=("$tier")
    fi
done

index_path="${evidence_dir}/index-$(date -u +%Y%m%dT%H%M%SZ).md"
{
    echo "# Tier Certification Evidence Index"
    echo
    echo "- Generated (UTC): $(date -u +%Y-%m-%dT%H:%M:%SZ)"
    echo
    echo "| Tier | Decision | Evidence Path |"
    echo "|---|---|---|"
    for path in "${generated[@]}"; do
        base="$(basename "$path")"
        tier_key="${base%%-*}"
        tier_upper="$(echo "$tier_key" | tr '[:lower:]' '[:upper:]')"
        decision="$(sed -n 's/^- Overall Decision: //p' "$path" | head -n 1)"
        if [[ -z "$decision" ]]; then
            decision="UNKNOWN"
        fi
        echo "| ${tier_upper} | ${decision} | ${path} |"
    done
} >"$index_path"

echo "Generated evidence artifacts:"
printf '%s\n' "${generated[@]}"
echo "$index_path"

if (( ${#unique_failed[@]} > 0 )); then
    echo "Tiers with failing checks: ${unique_failed[*]}" >&2
    if [[ "$strict" -eq 1 ]]; then
        exit 1
    fi
fi
