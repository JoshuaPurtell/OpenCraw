# WebChat Channel Usage Guide

Generated: 2026-02-07

This guide covers operational usage for the built-in WebChat channel.

## Channel contract

- Channel id: `webchat`
- Transport: WebSocket at `/ws`
- Sender identity: generated UUID returned in `hello` payload
- Thread identity: same as sender id
- Allowlist behavior: WebChat is always allowed by default (local/dev channel)

## Wire protocol

### Client -> server

- message:

```json
{"type":"message","content":"hello"}
```

- reaction:

```json
{"type":"reaction","emoji":"👍"}
```

### Server -> client

- handshake:

```json
{"type":"hello","sender_id":"<uuid>"}
```

- typing indicator:

```json
{"type":"typing","active":true}
```

- streaming delta:

```json
{"type":"delta","content":"partial chunk"}
```

- final assistant message:

```json
{"type":"message","content":"final response"}
```

## Running and connecting

Start server:

```bash
cd /Users/synth/OpenCraw
ANTHROPIC_API_KEY=... cargo run -p os-app -- serve --config ~/.opencraw/config.toml
```

Default websocket URL:

```text
ws://localhost:3000/ws
```

## Manual outbound send path

Use `recipient` as the `sender_id` from the `hello` payload:

```bash
curl -sS -X POST http://localhost:3000/api/v1/os/messages/send \
  -H 'content-type: application/json' \
  -d '{"channel":"webchat","recipient":"<sender_uuid>","message":"hello from API"}'
```

## Failure modes and exact meanings

- `webchat payload missing type`
  - Incoming JSON payload omitted `type`.
- `webchat message missing content`
  - `type=message` payload missing `content`.
- `webchat reaction missing emoji`
  - `type=reaction` payload missing `emoji`.
- `webchat unsupported message type`
  - Unsupported `type` value; connection is closed.
- `webchat connection not found for recipient_id=...`
  - Outbound send targeted stale/nonexistent socket session.

## Logging expectations

Normal operation includes:
- `gateway loop started`
- `inbound message received` with `channel_id=webchat`
- `assistant run started` / `assistant run completed`

WebSocket parse/payload issues include:
- `webchat received invalid json`
- `webchat invalid payload`
- `webchat unsupported message type`
