# OpenCraw

A personal AI assistant built on [Horizons](https://github.com/synth-laboratories/Horizons).

## Status: v0.1.0

This repo is a Rust workspace with a small, working slice of a multi-channel personal assistant:

- Channels:
  - WebChat (Axum WebSocket at `/ws`)
  - Telegram (Bot API long polling)
  - Discord (Gateway WS; mention-only in guild channels)
- Tools:
  - Shell execution (high risk)
  - Filesystem read/write under a configured root (write = medium risk)
  - Clipboard (stub)
  - Browser (stub)
- Sessions:
  - Per `(channel_id, sender_id)` session history
- Horizons integration:
  - Uses `horizons_core::core_agents` approval gates
  - Optionally wires Voyager-backed memory via Horizons memory traits
  - Mounts `horizons_rs` HTTP API router alongside OpenCraw routes

## Workspace

- `os-llm/`: minimal LLM client wrapper (OpenAI-compatible + Anthropic), including streaming + tool call normalization
- `os-tools/`: tool traits/specs + implementations (shell/filesystem/clipboard/browser)
- `os-channels/`: channel adapter traits + implementations (webchat/telegram/discord)
- `os-app/`: binary (`opencraw`) that wires config, tools, channels, sessions, and Horizons runtime

## Quickstart

Prereqs:

- `../Horizons` checked out next to this repo (OpenCraw depends on it via path dependencies)
- Rust toolchain installed

Run locally:

```bash
mkdir -p ~/.opencraw
cp config.example.toml ~/.opencraw/config.toml
# Set OPENAI_API_KEY or ANTHROPIC_API_KEY (or edit ~/.opencraw/config.toml)

cargo run -p os-app -- serve
```

WebChat:

- WebSocket: `ws://localhost:3000/ws`
- Client sends JSON like:

```json
{ "type": "message", "content": "hi" }
```

Docker:

```bash
docker compose up --build
```

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
