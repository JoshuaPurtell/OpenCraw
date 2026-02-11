# AGENTS.md

## Mandatory Preflight (Every Turn)

Before doing anything in this repository (analysis, commands, edits, tests, or planning), re-read these documents in order:

1. `~/.claude/CLAUDE.md`
2. This file (`AGENTS.md`)
3. `standards/rust.md`
4. `standards/foundation-gates.md`

## Hard Rules

- These documents are the source of truth for engineering behavior in this repo.
- Do not proceed with any task until they have been re-read for the current user prompt.
- If any file is missing or unreadable, stop and report it before taking further action.
- Governance surfaces are immutable in PRs (`standards/*`, `AGENTS.md`, `.github/workflows/*`, `docs/process/*`).
- Do not edit governance controls unless the task is explicitly foundation hardening.

## Mandatory Quality Gate (Any Code Change)

Before submitting any code change, run:

1. `cargo fmt --all -- --check`
2. `cargo check --workspace --all-targets --locked`
3. `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
4. `cargo test --workspace --all-targets --locked`
