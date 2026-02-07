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
mkdir -p ~/.opencraw
cp config.example.toml ~/.opencraw/config.toml
# Set OPENAI_API_KEY or ANTHROPIC_API_KEY (or edit ~/.opencraw/config.toml)

scripts/compose.sh up
```

This builds the image and starts OpenCraw + Postgres + Redis.

Run the Rust binary directly (for faster dev iteration):

```bash
# Start backing services only
scripts/compose.sh up postgres redis

# Run OpenCraw from source
cargo run -p os-app -- serve
```

OpenCraw runtime config now includes:

- `[runtime] mode = "dev" | "prod"` (`prod` is strict and fails fast on missing/invalid required env)
- `[runtime] data_dir = "data"` for local runtime state
- `[queue] mode = "followup" | "collect" | "steer" | "interrupt"` for lane behavior policy
- `[queue] max_concurrency`, `[queue] lane_buffer`, and `[queue] debounce_ms` for lane-aware dispatch tuning
- `[context] max_prompt_tokens`, `min_recent_messages`, `max_tool_chars` for token-aware history trimming
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
`channels.imessage.group_prefixes` (defaults in `config.example.toml`).

## License

MIT (see `LICENSE.md`).
