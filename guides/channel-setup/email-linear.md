# Channel Setup: Email and Linear

Generated: 2026-02-07

This document covers exact setup and verification for OpenCraw Email (Gmail) and Linear channels.

## Email (Gmail)

### 1. Prerequisites

- Gmail account with API access.
- OAuth access token with Gmail scopes sufficient for:
  - reading messages
  - modifying message labels (if `mark_processed_as_read = true`)
  - sending messages
- OpenCraw config at `~/.opencraw/config.toml`.

### 2. Config

```toml
[channels.email]
enabled = true
provider = "gmail"
gmail_access_token = "ya29...."
poll_interval_ms = 2000
query = "in:inbox is:unread"
start_from_latest = true
mark_processed_as_read = true

[security]
allow_all_senders = false
allowed_users = ["email:user@example.com"]
```

Notes:
- `provider` is strict and must be `"gmail"`.
- `start_from_latest = true` seeds at startup and only emits new messages after startup.
- `mark_processed_as_read = true` removes Gmail `UNREAD` label after OpenCraw ingests the message.

### 3. Allowlist format

Email sender identity is parsed from the Gmail `From` header.
Use either:

- raw sender id: `user@example.com`
- composite (recommended): `email:user@example.com`

### 4. Start OpenCraw

```bash
cd /Users/synth/OpenCraw
ANTHROPIC_API_KEY=... cargo run -p os-app -- serve --config ~/.opencraw/config.toml
```

### 5. Verify inbound and outbound

Inbound:
- Send a new email from an allowlisted address matching the configured query.

Outbound API test:

```bash
curl -sS -X POST http://localhost:3000/api/v1/os/messages/send \
  -H 'content-type: application/json' \
  -d '{"channel":"email","recipient":"user@example.com","message":"test from opencraw"}'
```

Optional threaded send format:
- `recipient = "thread:<gmail_thread_id>:user@example.com"`

### 6. Email troubleshooting

- `channels.email.provider must be 'gmail'`: set provider to `gmail`.
- `channels.email.gmail_access_token is required`: set a non-empty token.
- `gmail list/get/send failed`: token scope invalid/expired or API access issue.
- No inbound events: query does not match, sender not allowlisted, or startup seeded with `start_from_latest=true` and no new mail arrived.

## Linear

### 1. Prerequisites

- Linear API key (user token).
- Access to issues/comments in the target workspace.
- OpenCraw config at `~/.opencraw/config.toml`.

### 2. Config

```toml
[channels.linear]
enabled = true
api_key = "lin_api_..."
poll_interval_ms = 3000
team_ids = ["OPS"]
start_from_latest = true

[security]
allow_all_senders = false
allowed_users = ["linear:user_id"]
```

Notes:
- `team_ids` is optional; if set, it filters by team id/key/name.
- Inbound events are generated from new comments on viewer-assigned issues.
- `start_from_latest = true` seeds existing comments and emits only new comments after startup.

### 3. Allowlist format

Linear sender identity is comment `user.id`.
Use either:

- raw sender id: `user_id`
- composite (recommended): `linear:user_id`

### 4. Start OpenCraw

```bash
cd /Users/synth/OpenCraw
ANTHROPIC_API_KEY=... cargo run -p os-app -- serve --config ~/.opencraw/config.toml
```

### 5. Verify inbound and outbound

Inbound:
- Add a new comment to a viewer-assigned issue from an allowlisted Linear user.

Outbound API test:

```bash
curl -sS -X POST http://localhost:3000/api/v1/os/messages/send \
  -H 'content-type: application/json' \
  -d '{"channel":"linear","recipient":"<issue_id>","message":"test from opencraw"}'
```

`recipient` must be a Linear issue id.

### 6. Linear troubleshooting

- `channels.linear.api_key is required`: set a non-empty key.
- `linear graphql failed`: auth/network error.
- `linear graphql returned errors`: GraphQL validation/schema/auth issue.
- No inbound events: no new comments since startup seed, issue not assigned to viewer, team filter excludes issue, or sender not allowlisted.
