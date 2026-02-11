# OpenCraw

A personal AI assistant built on [Horizons](https://github.com/synth-laboratories/Horizons).

## Status: v0.1.0

This repo is a Rust workspace with a small, working slice of a multi-channel personal assistant:

- Channels:
  - WebChat (Axum WebSocket at `/ws`)
  - Telegram (Bot API long polling)
  - Discord (Gateway WS; mention-only in guild channels)
  - iMessage (macOS Messages DB poll + AppleScript send)
  - Email (Gmail API polling + send)
  - Linear (assigned-issue comment polling + comment send)
- Tools:
  - Shell execution (high risk)
  - Filesystem read/write under a configured root (write = medium risk)
  - Horizons memory tools (`memory.search`, `memory.summarize`) when memory is enabled
  - Clipboard (stub)
  - Browser (stub)
- Sessions:
  - Per `(channel_id, sender_id)` session history with Horizons ProjectDb persistence
- Horizons integration:
  - Uses `horizons_core::core_agents` approval gates
  - Optionally wires Voyager-backed memory via Horizons memory traits
  - Mounts `horizons_rs` HTTP API router alongside OpenCraw routes

## Workspace

- `os-llm/`: minimal LLM client wrapper (OpenAI-compatible + Anthropic), including streaming + tool call normalization
- `os-tools/`: tool traits/specs + implementations (shell/filesystem/clipboard/browser)
- `os-channels/`: channel adapter traits + implementations (webchat/telegram/discord/imessage/email/linear)
- `os-app/`: binary (`opencraw`) that wires config, tools, channels, sessions, and Horizons runtime

## Quickstart

Prereqs:

- Docker (for Postgres + Redis)
- Rust toolchain (for local dev builds)

Run with Docker (recommended):

```bash
cargo run -p os-app -- init
# Fill local config in ~/.opencraw/config.toml and ~/.opencraw/configs/*.toml
# (at minimum: ~/.opencraw/configs/keys.toml)

scripts/compose.sh up
```

This builds the image and starts OpenCraw + Postgres + Redis.

`opencraw init` is idempotent and never overwrites existing files in `~/.opencraw/`.

Run the Rust binary directly (for faster dev iteration):

```bash
# Start backing services only
scripts/compose.sh up postgres redis

# Run OpenCraw from source
cargo run -p os-app -- serve
```

Init templates live under:

- `config-templates/`

OpenCraw supports modular local config loading:

1. Load `~/.opencraw/config.toml` (base config)
2. Load `~/.opencraw/configs/*.toml` (provider/component fragments)

Valid fragment filenames are fixed (fail-fast on unknown names), including:

- `llm.toml`, `general.toml`, `runtime.toml`, `keys.toml`, `tools.toml`, `security.toml`, `queue.toml`, `context.toml`, `memory.toml`, `optimization.toml`, `automation.toml`, `skills.toml`
- `channel-webchat.toml`, `channel-telegram.toml`, `channel-discord.toml`, `channel-slack.toml`, `channel-matrix.toml`, `channel-signal.toml`, `channel-whatsapp.toml`, `channel-imessage.toml`, `channel-email.toml`, `channel-linear.toml`, `channel-external-plugins.toml`

Sender ACL is provider-local in channel fragments under `[channels.<provider>.access]` with:

- `mode = "pairing" | "allowlist" | "open"`
- `allowed_senders = [...]` (required when `mode = "allowlist"` and channel is enabled)

Keep secrets and personal IDs local under `~/.opencraw/` (never in repository-tracked files).

### Refresh Gmail Access Token

OpenCraw email currently expects a live Gmail access token in
`~/.opencraw/configs/channel-email.toml` (`channels.email.gmail_access_token`).

Set these in your repo-local `.env`:

```bash
OPENCRAW_GMAIL_OAUTH_CLIENT_ID="..."
OPENCRAW_GMAIL_OAUTH_CLIENT_SECRET="..."
OPENCRAW_GMAIL_OAUTH_REFRESH_TOKEN="..."
```

Then refresh + write token automatically:

```bash
scripts/refresh-gmail-access-token.sh
```

The script writes a config backup under `~/.opencraw/backups/`.

### Populate Linear Config

OpenCraw Linear expects a local channel config at
`~/.opencraw/configs/channel-linear.toml`.

Set at minimum in your repo-local `.env`:

```bash
OPENCRAW_LINEAR_API_KEY="lin_api_..."
# Preferred when you know human name/key but not UUID.
# Examples: "Synth", "SYNTH"
OPENCRAW_LINEAR_TEAM="your_team_name_or_key"
# Optional explicit override:
# OPENCRAW_LINEAR_DEFAULT_TEAM_ID="team_uuid_for_issue_create"
```

Then populate/update the Linear config automatically:

```bash
scripts/populate-linear-config.sh
```

Contract-check the live Linear GraphQL schema against OpenCraw expectations:

```bash
scripts/check-linear-contracts.sh
```

This validates Query/Mutation signatures and input object fields used by the
Linear tool (`issueCreate`, `projectCreate`, `issueUpdate`, `commentCreate`)
before you run provider workflows.

The populate script:

- Validates the Linear API key by querying teams.
- Resolves `default_team_id` from `OPENCRAW_LINEAR_TEAM` (name/key/id) when provided.
- Auto-selects `default_team_id` only when exactly one team is visible.
- Writes `[channels.linear.actions]` toggles into local config (all enabled by default), including:
  - `whoami`, `list_assigned`, `list_users`, `list_teams`, `list_projects`
  - `create_issue`, `create_project`, `update_issue`, `assign_issue`, `comment_issue`
- Writes `~/.opencraw/configs/channel-linear.toml`.
- Backs up any existing file to `~/.opencraw/backups/`.

OpenCraw runtime config now includes:

- `[llm] active_profile`, `fallback_profiles`, `failover_cooldown_*`, and `[llm.profiles.<name>]` (`provider`, `model`, optional `fallback_models`)
- `[runtime] mode = "dev" | "prod"` (`prod` is strict and fails fast on missing/invalid required env)
- `[runtime] data_dir = "data"` for local runtime state
- `[queue] mode = "followup" | "collect" | "steer" | "interrupt"` for lane behavior policy
- `[queue] max_concurrency`, `[queue] lane_buffer`, and `[queue] debounce_ms` for lane-aware dispatch tuning
- `[security] human_approval_timeout_seconds` for approval wait behavior (`0` waits indefinitely)
- `[context] max_prompt_tokens`, `min_recent_messages`, `max_tool_chars`, `tool_loops_max`, `tool_max_runtime_seconds`, `tool_no_progress_limit` for token-aware history trimming and loop/runtime safety bounds
- `[context] compaction_*` for Horizons-backed pre-compaction flush + summary rewrite (requires memory enabled)

Control-plane config APIs:

- `GET /api/v1/os/config/get`
- `POST /api/v1/os/config/apply` (full config payload)
- `POST /api/v1/os/config/patch` (`{ base_hash?, patch }`)

Memory APIs (when `[memory].enabled = true`):

- `POST /api/v1/os/memory/search`
- `POST /api/v1/os/memory/summarize`

### Local Horizons development

Horizons is pulled automatically as a git dependency. If you're actively hacking on
Horizons alongside OpenCraw, override with a local checkout:

```bash
cp .cargo/config.toml.example .cargo/config.toml
# Edit the paths in .cargo/config.toml to point to your Horizons checkout
```

WebChat:

- WebSocket: `ws://localhost:3000/ws`
- Client sends JSON like:

```json
{ "type": "message", "content": "hi" }
```

Server WebSocket events include:

- `{"type":"hello","sender_id":"..."}`
- `{"type":"typing","active":true|false}`
- `{"type":"delta","content":"..."}` (streaming chunks)
- `{"type":"message","content":"..."}` (final assistant message)

Docker:

```bash
scripts/compose.sh up
```

The compose helper detects `docker compose` vs `docker-compose`, builds the image,
and starts OpenCraw + Postgres + Redis. Other commands:

```bash
scripts/compose.sh build   # build only
scripts/compose.sh down    # stop everything
scripts/compose.sh logs    # tail logs (or: scripts/compose.sh logs opencraw)
scripts/compose.sh ps      # list containers
```

## CI (Rust)

Strict Rust checks run in GitHub Actions on PRs and pushes to `main`.

Run the same gate locally:

```bash
scripts/ci-rust.sh
```

This enforces:

- `cargo fmt --all -- --check`
- `cargo check --workspace --all-targets --locked`
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
- `cargo test --workspace --all-targets --locked`
- `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked`

## iMessage (macOS)

OpenCraw can integrate with iMessage on macOS using:

- Reads: `~/Library/Messages/chat.db` (polling for new messages)
- Sends: AppleScript via `osascript` controlling the Messages app

To enable:

1. In config (`~/.opencraw/config.toml`), set:
   - `[channels.imessage] enabled = true`
2. Grant macOS permissions:
   - Full Disk Access to the terminal running OpenCraw (so it can read `chat.db`)
   - Automation permission for the terminal to control “Messages” (for sending)

Safety note: in group chats, OpenCraw only responds to messages prefixed with one of
`channels.imessage.group_prefixes` (defaults in `config-templates/config.toml`).

## License

MIT (see `LICENSE.md`).
