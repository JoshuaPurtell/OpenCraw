# Linear Channel Usage Guide

Generated: 2026-02-07

This guide explains day-to-day usage of OpenCraw Linear after initial setup.

For setup/config, see:
- `/Users/synth/OpenCraw/guides/channel-setup/email-linear.md`

## Channel contract

- Channel id: `linear`
- Sender identity used for allowlist: Linear comment `user.id`
- Thread identity used by OpenCraw: Linear `issue.id`
- Inbound source: comments on issues assigned to the token owner

## API contract snapshots

Linear schema contracts are snapshotted in-repo at:

- `/Users/synth/OpenCraw/contracts/linear/`

Refresh snapshot:

```bash
scripts/fetch-linear-contracts.sh
```

## How inbound behavior works

On each poll cycle:
1. OpenCraw calls Linear GraphQL for viewer assigned issues.
2. It reads recent comments per issue.
3. New comments (not yet seen in this process) are emitted as inbound messages.
4. Queue lane key is effectively `linear::<sender_id>`.

Inbound content format passed to assistant:

```text
[OPS-123] Improve retry path
From: Alex Rivera

Can we adjust backoff for 429 responses?
```

## Sending outbound messages manually

Use the unified send endpoint; recipient must be issue id.

```bash
curl -sS -X POST http://localhost:3000/api/v1/os/messages/send \
  -H 'content-type: application/json' \
  -d '{"channel":"linear","recipient":"8f2f4fbb-...","message":"Investigating now."}'
```

OpenCraw sends this as a Linear comment via `commentCreate`.

## Allowlist usage

Recommended allowlist entries:

```toml
[security]
allow_all_senders = false
allowed_users = [
  "linear:usr_abc123",
  "linear:usr_def456"
]
```

Raw values also work, but prefixed form is preferred.

## Practical operating patterns

### Pattern: single-team agent

Filter to one or more teams:

```toml
[channels.linear]
team_ids = ["OPS", "Platform"]
```

Matching accepts team id, key, or name (case-insensitive).

### Pattern: avoid historical replay at startup

Use startup seeding (default):

```toml
[channels.linear]
start_from_latest = true
```

This emits only comments that appear after the process starts.

### Pattern: high responsiveness

Lower poll interval (tradeoff: more API traffic):

```toml
[channels.linear]
poll_interval_ms = 1500
```

## Production checks

### Health check: config validation

```bash
ANTHROPIC_API_KEY=... cargo run -p os-app -- doctor --config ~/.opencraw/config.toml
```

### Health check: one-shot send path

```bash
ANTHROPIC_API_KEY=... cargo run -p os-app -- send linear <issue_id> "smoke"
```

## Failure modes and exact meanings

- `channels.linear.api_key is required`
  - Linear channel enabled with empty key.
- `linear graphql failed`
  - Network/auth/transport failure (non-2xx).
- `linear graphql returned errors`
  - GraphQL-level errors in response.
- `recipient_id (Linear issue id) is required`
  - Empty recipient passed to send path.
- `linear commentCreate returned success=false`
  - Mutation accepted but not successful.

## Logging expectations

With default structured logs, you should see:
- `linear adapter seeded initial cursor`
- `linear poll cycle complete`
- `linear comment posted`

If polling exits, you’ll see:
- `linear poll loop exited`
