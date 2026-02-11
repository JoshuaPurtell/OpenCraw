# Verification Evidence Contract

- Status: accepted
- Owner: platform
- Last-Reviewed: 2026-02-11
- Scope: change verification evidence and acceptance decisions
- Depends-On: `AGENTS.md`, `standards/foundation-gates.md`, `standards/verification-governance.md`

## 1. Core Rule

1. Change acceptance is based on evidence quality, not runner brand.
2. CI success is one valid evidence source, not policy authority.
3. Runner failure does not waive required verification.

## 2. Required Evidence Bundle

Every non-trivial change MUST produce an evidence bundle containing:

1. Change classification (`foundation-hardening`, `feature-delivery`, `maintenance`).
2. Commands executed.
3. Runner and environment identity.
4. Pass/fail outcome per command.
5. Durable artifact pointers.
6. Explicit skipped checks and reason.

## 3. Class-Specific Evidence

1. `foundation-hardening`:
   Must show governance diff rationale and full applicable gate results.
2. `feature-delivery`:
   Must show behavior validation plus required quality gates.
3. `maintenance`:
   Must show no-regression proof proportional to touched surfaces.

## 4. Rust Change Evidence

For Rust code changes, evidence MUST include:

1. `cargo fmt --all -- --check`
2. `cargo check --workspace --all-targets --locked`
3. `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
4. `cargo test --workspace --all-targets --locked`

Additional required gates from `standards/foundation-gates.md` also apply.

## 5. Multi-Agent Evidence Contract

1. Each lane is accountable for evidence of files in its allowlist.
2. Integration lane verifies cross-lane evidence completeness for shared merges.
3. Tier promotion requires certification artifacts and explicit pass/fail outcomes.

## 6. Waiver and Expiry Contract

1. Waivers MUST include owner, scope, expiry, compensating control, rollback plan.
2. Expired waivers are invalid and block merge.
3. All waiver usage MUST be traceable in review artifacts.
