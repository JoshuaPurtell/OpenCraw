# Email Channel Usage Guide (Gmail)

Generated: 2026-02-07

This guide explains day-to-day usage of OpenCraw Email after initial setup.

For setup/config, see:
- `/Users/synth/OpenCraw/plans/10-channel-setup-email-linear.md`

## Channel contract

- Channel id: `email`
- Sender identity used for allowlist: parsed Gmail `From` address (e.g., `user@example.com`)
- Thread identity used by OpenCraw: Gmail `threadId`
- Inbound scope: unread messages matching configured Gmail query

## How inbound behavior works

When a new matched message arrives:
1. OpenCraw polls Gmail API (`messages.list` + `messages.get`).
2. It emits one inbound message into the lane for `email::<sender>`.
3. If `mark_processed_as_read = true`, OpenCraw removes `UNREAD` from that message.
4. Assistant reply is sent back through Gmail `messages.send`.

Inbound content format passed to assistant:

```text
From: Jane Doe <jane@example.com>
Subject: Build status

Can you summarize the last deployment errors?
```

## Sending outbound messages manually

Use the unified send endpoint:

```bash
curl -sS -X POST http://localhost:3000/api/v1/os/messages/send \
  -H 'content-type: application/json' \
  -d '{"channel":"email","recipient":"user@example.com","message":"Hello from OpenCraw"}'
```

### Replying to an existing Gmail thread

Use recipient format:

```text
thread:<gmail_thread_id>:<email>
```

Example:

```bash
curl -sS -X POST http://localhost:3000/api/v1/os/messages/send \
  -H 'content-type: application/json' \
  -d '{"channel":"email","recipient":"thread:198f35a1d4a8:user@example.com","message":"Following up on this thread."}'
```

## Allowlist usage

Recommended allowlist entries:

```toml
[security]
allow_all_senders = false
allowed_users = [
  "email:user@example.com",
  "email:alerts@company.com"
]
```

Raw values (without prefix) also work, but prefixed form is preferred.

## Practical operating patterns

### Pattern: triage mailbox

Use query filter to narrow traffic:

```toml
[channels.email]
query = "in:inbox is:unread (subject:incident OR from:alerts@company.com)"
```

### Pattern: retain unread for human review

Disable mark-read behavior:

```toml
[channels.email]
mark_processed_as_read = false
```

### Pattern: start cold without replay

Use startup seeding (default):

```toml
[channels.email]
start_from_latest = true
```

This avoids back-processing old inbox messages on restart.

## Production checks

### Health check: config validation

```bash
ANTHROPIC_API_KEY=... cargo run -p os-app -- doctor --config ~/.opencraw/config.toml
```

### Health check: one-shot send path

```bash
ANTHROPIC_API_KEY=... cargo run -p os-app -- send email user@example.com "smoke"
```

## Failure modes and exact meanings

- `channels.email.provider must be 'gmail'`
  - Config has unsupported provider value.
- `channels.email.gmail_access_token is required`
  - Email channel enabled with empty token.
- `gmail list messages failed` / `gmail get message failed` / `gmail send failed`
  - Gmail API auth/scope/network issue.
- `invalid recipient format, expected thread:<thread_id>:<email>`
  - Bad threaded recipient format.

## Logging expectations

With default structured logs, you should see:
- `email adapter seeded initial cursor`
- `email poll cycle complete`
- `email message sent`

If polling exits, you’ll see:
- `email poll loop exited`
