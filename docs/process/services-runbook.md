# OpenCraw Services Runbook (No UI) + Code Proof

Updated: 2026-02-10 (UTC)
Scope: service-first operation (Telegram/Gmail/etc.), not dashboard-first.

## 1) Normie How-To (Run It, Use It)

### A. Boot OpenCraw once

1. Prepare config.

```bash
mkdir -p ~/.opencraw
cp config-templates/config.toml ~/.opencraw/config.toml
```

2. Add at least one model key in `~/.opencraw/config.toml` (`[keys]`), for example:
- `openai_api_key = "..."`, or
- `anthropic_api_key = "..."`

3. Start dependencies (optional if already running externally):

```bash
# cd to repo root
scripts/compose.sh up postgres redis
```

4. Run OpenCraw:

```bash
# cd to repo root
cargo run -p os-app -- serve --config ~/.opencraw/config.toml
```

5. Sanity checks in another terminal:

```bash
curl -sS http://127.0.0.1:3000/api/v1/os/health | jq
curl -sS http://127.0.0.1:3000/api/v1/os/channels | jq
cargo run -p os-app -- status --config ~/.opencraw/config.toml
```

What should work immediately with default config:
- `webchat` channel registration.
- Core health/config/control endpoints.
- CLI commands: `serve`, `doctor`, `status`, `send`.

### B. Enable and test each channel service

Use this per-service loop:

1. Set `[channels.<service>].enabled = true` and required secrets in `~/.opencraw/config.toml`.
2. Restart OpenCraw.
3. Confirm registration:

```bash
curl -sS http://127.0.0.1:3000/api/v1/os/channels | jq
curl -sS http://127.0.0.1:3000/api/v1/os/channels/<service>/probe | jq
```

4. Test outbound from CLI:

```bash
# cd to repo root
cargo run -p os-app -- send <service> "<recipient>" "hello from opencraw"
```

#### Service-by-service checklist

| Service | Config block | Required config | Recipient format for `send` |
|---|---|---|---|
| WebChat | `[channels.webchat]` | `enabled=true`, `port=3000` | WebSocket sender ID from `/ws` session |
| Telegram | `[channels.telegram]` | `bot_token` | Telegram chat ID |
| Discord | `[channels.discord]` | `bot_token` | Discord channel ID |
| Slack | `[channels.slack]` | `bot_token`, `channel_ids[]` | Slack channel ID |
| Matrix | `[channels.matrix]` | `homeserver_url`, `access_token`, `user_id`, `room_ids[]` | Matrix room ID |
| Signal | `[channels.signal]` | `api_base_url`, `account` (+ optional token) | E.164 number or `group:<group_id>` |
| WhatsApp Cloud | `[channels.whatsapp]` | `access_token`, `phone_number_id`, `webhook_verify_token` (+ optional `app_secret`) | E.164 phone number |
| iMessage | `[channels.imessage]` | `source_db` (macOS Messages DB path) | iMessage handle/chat target |
| Email (Gmail) | `[channels.email]` | `gmail_access_token` | `user@example.com` or `thread:<thread_id>:user@example.com` |
| Linear | `[channels.linear]` | `api_key` | Linear issue ID |
| External plugin | `[[channels.external_plugins]]` | `id`, `send_url` (+ optional `poll_url`, `auth_token`) | Plugin-defined recipient string |

### C. Core service APIs (beyond channels)

Health and discovery:

```bash
curl -sS http://127.0.0.1:3000/api/v1/os/health | jq
curl -sS http://127.0.0.1:3000/api/v1/os/config/network | jq
curl -sS http://127.0.0.1:3000/api/v1/os/config/discovery | jq
```

Config control:

```bash
curl -sS http://127.0.0.1:3000/api/v1/os/config/get | jq
```

Send API (equivalent to CLI `send`):

```bash
curl -sS -X POST http://127.0.0.1:3000/api/v1/os/messages/send \
  -H 'content-type: application/json' \
  -d '{"channel":"telegram","recipient":"<chat_id>","message":"hello via api"}' | jq
```

Sessions:

```bash
curl -sS http://127.0.0.1:3000/api/v1/os/sessions | jq
```

Automation:

```bash
curl -sS http://127.0.0.1:3000/api/v1/os/automation/status | jq
curl -sS http://127.0.0.1:3000/api/v1/os/automation/jobs | jq
```

Skills:

```bash
curl -sS http://127.0.0.1:3000/api/v1/os/skills | jq
curl -sS "http://127.0.0.1:3000/api/v1/os/skills/search?q=telegram" | jq
```

Memory (only when `[memory].enabled = true`):

```bash
curl -sS -X POST http://127.0.0.1:3000/api/v1/os/memory/search \
  -H 'content-type: application/json' \
  -d '{"channel_id":"telegram","sender_id":"123456","query":"recent asks","limit":5}' | jq
```

### D. Auth headers for mutating endpoints (only when enforced)

In strict mode (`runtime.mode="prod"` or configured control API tokens), mutating endpoints require:
- `x-org-id: <uuid>`
- `Authorization: Bearer <token>` with proper scope

Template:

```bash
ORG_ID="<uuid>"
TOKEN="<control-api-token>"
curl -sS -X POST http://127.0.0.1:3000/api/v1/os/config/patch \
  -H "x-org-id: $ORG_ID" \
  -H "Authorization: Bearer $TOKEN" \
  -H 'content-type: application/json' \
  -d '{"patch":{"automation":{"enabled":true}}}' | jq
```

## 2) Code Proof + How It Works

### A. Service wiring proof (what exists in code)

- CLI entrypoints (`serve`, `doctor`, `status`, `send`): `os-app/src/main.rs:89`, `os-app/src/main.rs:90`, `os-app/src/main.rs:91`, `os-app/src/main.rs:92`
- Channel registry includes webchat/telegram/discord/slack/matrix/signal/whatsapp/imessage/email/linear:
  `os-app/src/channel_plugins.rs:40`, `os-app/src/channel_plugins.rs:41`, `os-app/src/channel_plugins.rs:42`, `os-app/src/channel_plugins.rs:43`, `os-app/src/channel_plugins.rs:44`, `os-app/src/channel_plugins.rs:45`, `os-app/src/channel_plugins.rs:46`, `os-app/src/channel_plugins.rs:47`, `os-app/src/channel_plugins.rs:48`, `os-app/src/channel_plugins.rs:49`
- External plugin channel support: `config-templates/config.toml:141`, `os-app/src/channel_plugins.rs:110`, `os-app/src/channel_plugins.rs:121`
- Capability matrix (`streaming/typing/reactions`) surfaced by `/channels`: `os-app/src/channel_plugins.rs:56`, `os-app/src/channel_plugins.rs:361`, `os-app/src/routes/channels.rs:11`

### B. API surface proof (service control plane)

- Health route: `os-app/src/routes/health.rs:9`
- Config routes: `os-app/src/routes/config.rs:19`, `os-app/src/routes/config.rs:22`, `os-app/src/routes/config.rs:23`
- Automation routes: `os-app/src/routes/automation.rs:13`, `os-app/src/routes/automation.rs:22`, `os-app/src/routes/automation.rs:27`
- Channels list/probe routes: `os-app/src/routes/channels.rs:11`, `os-app/src/routes/channels.rs:12`
- Send route: `os-app/src/routes/messages.rs:16`
- Sessions routes: `os-app/src/routes/sessions.rs:21`, `os-app/src/routes/sessions.rs:23`
- Skills routes: `os-app/src/routes/skills.rs:45`, `os-app/src/routes/skills.rs:47`
- Memory routes: `os-app/src/routes/memory.rs:27`, `os-app/src/routes/memory.rs:28`

### C. Channel contract proof (recipient semantics)

- Telegram send uses `chat_id`: `os-channels/src/telegram.rs:64`, `os-channels/src/telegram.rs:67`
- Discord send posts to `/channels/{recipient_id}/messages`: `os-channels/src/discord.rs:56`, `os-channels/src/discord.rs:57`
- Slack send requires Slack channel ID: `os-channels/src/slack.rs:97`, `os-channels/src/slack.rs:100`
- Matrix send requires room ID: `os-channels/src/matrix.rs:107`, `os-channels/src/matrix.rs:110`
- Signal send accepts phone or `group:<group_id>`: `os-channels/src/signal.rs:95`, `os-channels/src/signal.rs:99`, `os-channels/src/signal.rs:113`
- WhatsApp send requires E.164 recipient: `os-channels/src/whatsapp.rs:54`, `os-channels/src/whatsapp.rs:57`
- iMessage recipient required: `os-channels/src/imessage.rs:87`, `os-channels/src/imessage.rs:90`
- Email recipient supports thread format: `os-channels/src/email.rs:98`, `os-channels/src/email.rs:317`
- Linear recipient is issue ID: `os-channels/src/linear.rs:122`, `os-channels/src/linear.rs:125`
- Plugin recipient forwarded generically: `os-channels/src/http_plugin.rs:115`, `os-channels/src/http_plugin.rs:118`

### D. Security proof (why mutating routes may require headers/tokens)

- Mutating auth policy fields/scopes: `os-app/src/http_auth.rs:53`, `os-app/src/http_auth.rs:117`
- `x-org-id` UUID requirement in strict mode: `os-app/src/http_auth.rs:180`, `os-app/src/http_auth.rs:241`
- Bearer token + scoped enforcement: `os-app/src/http_auth.rs:248`, `os-app/src/http_auth.rs:261`
- Config-side token pool and exemptions: `config-templates/config.toml:220`, `config-templates/config.toml:230`

### E. WhatsApp webhook proof (verify + signed ingest)

- Webhook route mounted: `os-app/src/channel_plugins.rs:399`
- Verification handler: `os-app/src/channel_plugins.rs:412`
- Optional `x-hub-signature-256` validation: `os-app/src/channel_plugins.rs:437`, `os-app/src/channel_plugins.rs:568`

### F. Current certification proof (fresh run)

Fresh runs executed on 2026-02-10:

- T1 evidence: `docs/process/parallel-agents/certification/evidence/t1-20260210T182633Z.md` (Pass 16 / Fail 0)
- T2 evidence: `docs/process/parallel-agents/certification/evidence/t2-20260210T182649Z.md` (Pass 16 / Fail 0)
- T3 evidence: `docs/process/parallel-agents/certification/evidence/t3-20260210T182703Z.md` (Pass 16 / Fail 0)
- T4 evidence: `docs/process/parallel-agents/certification/evidence/t4-20260210T182718Z.md` (Pass 16 / Fail 0)

Certification harness source:
- `scripts/parity/check-tier-certification.sh`

### G. Practical interpretation (what should work as-is now)

Based on route wiring, adapter implementations, and current certification evidence:

- Core CLI and HTTP control plane are wired and runnable.
- All tiered channel families are implemented and pass current behavior-based certification checks.
- OpenCraw is ready for service-first validation (Telegram/Gmail/etc.) without dashboard work.
- Remaining work should focus on production hardening and operator workflows, not missing channel primitives.
