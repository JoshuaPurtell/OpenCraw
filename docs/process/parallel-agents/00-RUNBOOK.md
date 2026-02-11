# Parallel Agent Runbook (20-Agent Mode)

Purpose: run 20 Codex agents in parallel without stepping on each other.

## Core Rule

Each agent owns only its allowlisted files plus its own status file.

If a change is needed outside the allowlist:

1. Do not edit the file.
2. Append a request to `docs/process/parallel-agents/00-INTEGRATION-REQUESTS.md`.

## Hotspot Files (Integration Lane Only)

- `os-app/src/channel_plugins.rs`
- `os-app/src/config.rs`
- `os-app/src/server.rs`
- `os-app/src/main.rs`
- `os-app/src/routes/mod.rs`
- `os-channels/src/lib.rs`
- `os-channels/src/types.rs`
- `os-channels/src/traits.rs`
- `os-tools/src/lib.rs`
- `os-tools/src/traits.rs`
- `Cargo.toml`
- `Cargo.lock`
- `config-templates/config.toml`

## Branch Naming

- Use `codex/lane-<nn>-<short-name>`.
- Example: `codex/lane-03-telegram`.

## Required Update Discipline

Each agent must update only:

1. Its own status file in `docs/process/parallel-agents/status/`.
2. The integration request log (append-only) if needed.

Only Agent 01 updates:

- `docs/process/parallel-agents/00-COORDINATION.md`

## Done Criteria Per Lane

1. Code changes are limited to lane allowlist.
2. Tests/checks for touched crate(s) pass.
3. Status file includes:
   - completed work
   - test evidence
   - open integration requests

