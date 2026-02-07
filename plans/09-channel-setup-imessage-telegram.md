# Channel Setup: iMessage and Telegram

Generated: 2026-02-07

This document gives exact setup and verification steps for running OpenCraw with iMessage and Telegram.

## iMessage (exact setup)

### 1. Prerequisites

- macOS host with the Messages app signed in.
- `~/Library/Messages/chat.db` accessible.
- OpenCraw config file at `~/.opencraw/config.toml`.
- A valid LLM key (`ANTHROPIC_API_KEY` or `OPENAI_API_KEY`) and model in config.

### 2. Config

Use this in `~/.opencraw/config.toml`:

```toml
[channels.webchat]
enabled = true
port = 3000

[channels.imessage]
enabled = true
source_db = "~/Library/Messages/chat.db"
poll_interval_ms = 1500
start_from_latest = true
group_prefixes = ["@opencraw", "opencraw"]

[security]
allow_all_senders = false
allowed_users = ["imessage:+14155551212"]
```

Notes:
- `start_from_latest = true` means OpenCraw ignores old history and starts from newest row at startup.
- In group chats, OpenCraw only responds when the message starts with one of `group_prefixes`.

### 3. macOS permissions

Grant both:

1. Full Disk Access for the terminal app running OpenCraw.
2. Automation permission for that terminal app to control Messages.

Without these, reads and/or sends will fail.

### 4. Allowlist format (important)

For iMessage, sender identity comes from the `handle.id` value in Messages DB.
Use either:

- raw sender id: `+14155551212`
- composite: `imessage:+14155551212`

Composite form is recommended for clarity.

### 5. Start OpenCraw

```bash
cd /Users/synth/OpenCraw
ANTHROPIC_API_KEY=... cargo run -p os-app -- serve --config ~/.opencraw/config.toml
```

### 6. Verify inbound and outbound

Inbound:
- Send yourself a DM iMessage from an allowlisted sender.
- For group chats, start message with prefix, e.g. `@opencraw hi`.

Outbound API test:

```bash
curl -sS -X POST http://localhost:3000/api/v1/os/messages/send \
  -H 'content-type: application/json' \
  -d '{"channel":"imessage","recipient":"+14155551212","message":"test from opencraw"}'
```

For group sends, `recipient` should be the chat id (`chat_guid`) for that thread.

### 7. iMessage troubleshooting

- `channels.imessage.source_db is required`: set `source_db`.
- No inbound messages: check Full Disk Access and `allow_all_senders`/`allowed_users`.
- No group replies: check prefixes and ensure prefix is at message start.
- Send failures: check Automation permission for Messages.

## Telegram (same level of setup)

### 1. Prerequisites

- Telegram bot token from BotFather.
- Bot has started chat with user/group (user must send `/start` for DMs).
- OpenCraw config at `~/.opencraw/config.toml`.

### 2. Config

```toml
[channels.webchat]
enabled = true
port = 3000

[channels.telegram]
enabled = true
bot_token = "123456:ABCDEF..."

[security]
allow_all_senders = false
allowed_users = ["telegram:123456789"]
```

You can also provide token via env var `TELEGRAM_BOT_TOKEN`.

### 3. Allowlist format (important)

For Telegram, `sender_id` is the numeric `from.id` converted to string.
Use either:

- raw sender id: `123456789`
- composite: `telegram:123456789`

Composite is recommended.

### 4. Start OpenCraw

```bash
cd /Users/synth/OpenCraw
ANTHROPIC_API_KEY=... cargo run -p os-app -- serve --config ~/.opencraw/config.toml
```

### 5. Verify inbound and outbound

Inbound:
- DM your bot from the allowlisted Telegram account.
- For groups, ensure the message is text (non-text updates are ignored/fail for message path).

Outbound API test:

```bash
curl -sS -X POST http://localhost:3000/api/v1/os/messages/send \
  -H 'content-type: application/json' \
  -d '{"channel":"telegram","recipient":"123456789","message":"test from opencraw"}'
```

`recipient` is Telegram `chat_id`.
- In private chats this is usually the user chat id.
- In groups this is the group chat id.

### 6. Telegram troubleshooting

- `telegram getUpdates failed`: token invalid or network issue.
- No replies in DM: user did not `/start` bot yet.
- No replies despite inbound: sender not allowlisted (`allowed_users`) and `allow_all_senders=false`.
- Send fails with `status=... body=...`: wrong `chat_id`, bot not in chat, or chat permissions issue.

## Quick capability check

- iMessage: implemented and usable now on macOS.
- Telegram: implemented and usable now.
- Email + Linear: implemented in OpenCraw channel layer; see `/Users/synth/OpenCraw/plans/10-channel-setup-email-linear.md` for exact setup.
