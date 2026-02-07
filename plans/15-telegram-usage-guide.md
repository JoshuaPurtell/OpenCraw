# Telegram Channel Usage Guide

Generated: 2026-02-07

This guide covers operational usage for Telegram.

For setup details, see:
- `/Users/synth/OpenCraw/plans/09-channel-setup-imessage-telegram.md`

## Channel contract

- Channel id: `telegram`
- Inbound transport: Bot API long polling (`getUpdates`)
- Outbound transport: Bot API send (`sendMessage`)
- Sender identity used for allowlist: Telegram `from.id` as string
- Thread identity: Telegram `chat.id`

## Inbound behavior

- Polls with `allowed_updates=["message","message_reaction"]`.
- `message` events require text; missing text is treated as error.
- `message_reaction` events are mapped to reaction inbound messages.

DM vs group:
- DM: `chat.type = private`
- Group: non-private; still uses `chat.id` as thread id

## Outbound behavior

Use recipient as Telegram `chat_id`:

```bash
curl -sS -X POST http://localhost:3000/api/v1/os/messages/send \
  -H 'content-type: application/json' \
  -d '{"channel":"telegram","recipient":"123456789","message":"hello"}'
```

## Allowlist usage

Recommended:

```toml
[security]
allow_all_senders = false
allowed_users = ["telegram:123456789"]
```

Raw sender ids also match.

## Production checks

```bash
ANTHROPIC_API_KEY=... cargo run -p os-app -- doctor --config ~/.opencraw/config.toml
ANTHROPIC_API_KEY=... cargo run -p os-app -- send telegram 123456789 "smoke"
```

## Failure modes and exact meanings

- `channels.telegram.bot_token is required`
  - Telegram enabled with empty token.
- `telegram getUpdates failed: status=... body=...`
  - Token invalid/network/API failure.
- `telegram message missing text`
  - Received message update lacked text payload.
- `telegram message missing sender`
  - Update missing `from` sender.
- `telegram send failed: status=... body=...`
  - Invalid `chat_id`, bot not allowed, or API failure.

## Logging expectations

Normal:
- `inbound message received` with `channel_id=telegram`
- `assistant run started` / `assistant run completed`

Fatal polling errors:
- `telegram poll loop exited`
