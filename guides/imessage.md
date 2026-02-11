# iMessage Channel Usage Guide

Generated: 2026-02-07

This guide covers operational usage for iMessage.

For setup and permissions, see:
- `/Users/synth/OpenCraw/guides/channel-setup/imessage-telegram.md`

## Channel contract

- Channel id: `imessage`
- Inbound source: macOS Messages SQLite (`chat.db`)
- Outbound transport: AppleScript (`osascript`) via Messages app
- Sender identity used for allowlist: `handle.id` from Messages DB
- Thread identity: `chat_guid`

## Inbound behavior

- Polls DB by increasing message `ROWID`.
- If `start_from_latest = true`, startup seeds to current max row and only processes new rows.
- Skips outbound/self messages (`is_from_me != 0`).
- For group chats (`chat_guid` indicates group), replies are gated by configured prefixes.

Prefix behavior:
- If prefix matches start of message, prefix is stripped before assistant sees content.
- If no prefix match in group chat, message is ignored.

## Outbound behavior

`send` supports:
- direct handle targets (phone/email)
- chat targets (chat id / full handle containing `chat`)

API send example:

```bash
curl -sS -X POST http://localhost:3000/api/v1/os/messages/send \
  -H 'content-type: application/json' \
  -d '{"channel":"imessage","recipient":"+14155551212","message":"hello"}'
```

Group thread send example:

```bash
curl -sS -X POST http://localhost:3000/api/v1/os/messages/send \
  -H 'content-type: application/json' \
  -d '{"channel":"imessage","recipient":"chat123456789","message":"hello group"}'
```

## Allowlist usage

Recommended:

```toml
[security]
allow_all_senders = false
allowed_users = ["imessage:+14155551212"]
```

Raw sender ids also match.

## Production checks

```bash
ANTHROPIC_API_KEY=... cargo run -p os-app -- doctor --config ~/.opencraw/config.toml
ANTHROPIC_API_KEY=... cargo run -p os-app -- send imessage +14155551212 "smoke"
```

## Failure modes and exact meanings

- `channels.imessage.source_db is required`
  - iMessage channel enabled with missing DB path.
- `imessage message missing sender handle_id`
  - DB row missing sender handle.
- `imessage message missing text` or `imessage message has empty text content`
  - Inbound row has no usable text.
- `imessage message missing chat_guid`
  - DB row missing thread identity.
- send failures from `osascript`
  - Missing Automation permission or invalid recipient target.

## Logging expectations

Normal:
- `imessage poll loop exited` only on fatal loop error
- `inbound message received` with `channel_id=imessage`

If queue closes:
- `imessage inbound queue closed`
