# OpenCraw Control UI Plan

Date: 2026-02-06

## Context

The web frontend is currently a single-file WebChat client (`App.tsx`). The
backend already exposes REST endpoints for config, channels, sessions, memory,
skills, messages, and health. None of these are surfaced in the UI.

OpenClaw's Control UI is the reference target: a browser-based admin dashboard
with chat, config editor, channel status, session management, skills browser,
log viewer, and debug tools — all over a single WebSocket connection.

OpenCraw's transport is different (REST + a separate WebSocket for chat) but the
UI surface should converge on the same feature set, scoped to what the backend
actually supports today.

## What the Backend Supports Today

| Route | Method | Path | What it does |
|---|---|---|---|
| Health | GET | `/api/v1/os/health` | Returns `{ status: "ok" }` |
| Config get | GET | `/api/v1/os/config/get` | Full config snapshot + base_hash (keys redacted) |
| Config apply | POST | `/api/v1/os/config/apply` | Full config replacement, persists to disk |
| Config patch | POST | `/api/v1/os/config/patch` | JSON merge-patch with optimistic concurrency |
| Channels | GET | `/api/v1/os/channels` | List active channel adapters |
| Sessions list | GET | `/api/v1/os/sessions` | All sessions with metadata, sorted by last_active |
| Session delete | DELETE | `/api/v1/os/sessions/{id}` | Delete session by UUID |
| Messages send | POST | `/api/v1/os/messages/send` | Send outbound message to a channel |
| Memory search | POST | `/api/v1/os/memory/search` | Semantic search over long-term memory |
| Memory summarize | POST | `/api/v1/os/memory/summarize` | Time-windowed summary generation |
| Skills install | POST | `/api/v1/os/skills/install` | Register a skill in memory |
| Skills search | GET | `/api/v1/os/skills/search?q=` | Search installed skills |
| WebSocket | WS | `/ws` | Chat protocol (hello, message, typing, deltas) |

### Config Shape (Sections)

```
general       model, system_prompt
runtime       mode (dev|prod), data_dir
keys          openai_api_key, anthropic_api_key (redacted in API)
channels      webchat, telegram, discord, imessage (each with enabled + settings)
tools         shell, filesystem, browser, clipboard (booleans)
security      shell_approval, browser_approval, filesystem_write_approval,
              allowed_users[], allow_all_senders
queue         mode (followup|collect|steer|interrupt), max_concurrency, lane_buffer
context       max_prompt_tokens, min_recent_messages, max_tool_chars,
              compaction_enabled + settings
memory        enabled
optimization  enabled, schedule (cron)
```

### Session Shape

```
id, channel_id, sender_id, created_at, last_active, messages (count)
```

Full session includes: history (ChatMessage[]), usage_totals, show_thinking,
show_tool_calls, last_assistant_message_id, last_user_message_id.

## Architecture

### Tech Stack (Unchanged)

- React 19, TypeScript 5.9, Tailwind v4, Bun bundler
- No router library — use a simple state-driven view switcher
- No state management library — React state + context is sufficient at this scale

### Layout Shell

```
┌──────────────────────────────────────────────┐
│  Sidebar (nav)  │  View Content              │
│                 │                             │
│  [Chat]  ●      │  (selected view renders    │
│  [Sessions]     │   here, full height)        │
│  [Config]       │                             │
│  [Channels]     │                             │
│  [Memory]       │                             │
│  [Skills]       │                             │
│  ───────        │                             │
│  [Health]       │                             │
│                 │                             │
│  OC  v0.1       │                             │
└──────────────────────────────────────────────┘
```

- Sidebar: fixed-width (~220px), full height, icon + label nav items
- Active item highlighted; badge for unread/status
- Collapsible to icon-only on small screens
- View content: fills remaining space
- Chat view: keeps existing h-screen flex layout (toolbar removed, sidebar replaces it)

### File Structure

```
web/src/
  main.tsx                      # entry (unchanged)
  App.tsx                       # shell: sidebar + view router
  api.ts                        # all REST/WS client functions
  types.ts                      # shared types (config, session, etc.)
  views/
    ChatView.tsx                # current chat UI (extracted from App.tsx)
    SessionsView.tsx            # session list + delete
    ConfigView.tsx              # config editor
    ChannelsView.tsx            # channel status
    MemoryView.tsx              # memory search + summarize
    SkillsView.tsx              # skills search + install
    HealthView.tsx              # health status + debug
  components/
    Sidebar.tsx                 # nav sidebar
    StatusChip.tsx              # extracted from App.tsx
    MessageRow.tsx              # extracted from App.tsx
```

## Views

### 1. Chat View (refactor of current App.tsx)

Extract chat logic from App.tsx into a standalone view. WebSocket lifecycle
stays the same. The toolbar content (status chip, reconnect) moves into the
sidebar or a slim inline bar within the chat view.

**What it shows:**
- Chat log with message bubbles (you/assistant/system)
- Composer (input + send)
- Connection status
- Reconnect action

**No new backend work needed.**

### 2. Sessions View

**What it shows:**
- Table/list of all sessions: channel, sender, created, last active, message count
- Delete button per session (with confirmation)
- Auto-refresh on interval or manual refresh button

**API calls:**
- `GET /api/v1/os/sessions` — list
- `DELETE /api/v1/os/sessions/{id}` — delete

**No new backend work needed.**

### 3. Config View

**What it shows:**
- Organized config sections (general, runtime, channels, tools, security,
  queue, context, memory, optimization)
- Each section is a collapsible card with form fields
- Field types: text input, number input, toggle, select/dropdown, tag list
- API keys show as redacted; editing replaces the value
- Save button sends `config.patch` with current `base_hash`
- Stale-hash error shown inline ("config changed elsewhere, refresh to retry")
- "Last updated" timestamp + config file path shown

**API calls:**
- `GET /api/v1/os/config/get` — load current config + base_hash
- `POST /api/v1/os/config/patch` — save changes

**Field mapping:**

| Section | Fields | Input Type |
|---|---|---|
| General | model | text |
| General | system_prompt | textarea |
| Runtime | mode | select: dev/prod |
| Runtime | data_dir | text |
| Keys | openai_api_key | password (redacted) |
| Keys | anthropic_api_key | password (redacted) |
| Channels > WebChat | enabled, port | toggle, number |
| Channels > Telegram | enabled, bot_token | toggle, password |
| Channels > Discord | enabled, bot_token | toggle, password |
| Channels > iMessage | enabled, source_db, poll_interval_ms, start_from_latest, group_prefixes | toggle, text, number, toggle, tag list |
| Tools | shell, filesystem, browser, clipboard | toggles |
| Security | shell_approval, browser_approval, filesystem_write_approval | select: human/ai/auto |
| Security | allowed_users | tag list |
| Security | allow_all_senders | toggle |
| Queue | mode | select: followup/collect/steer/interrupt |
| Queue | max_concurrency, lane_buffer | number |
| Context | max_prompt_tokens, min_recent_messages, max_tool_chars | number |
| Context | compaction_enabled | toggle |
| Context | compaction_trigger_tokens, compaction_retain_messages, compaction_flush_max_chars | number |
| Context | compaction_horizon | text |
| Memory | enabled | toggle |
| Optimization | enabled | toggle |
| Optimization | schedule | text (cron) |

**No new backend work needed.**

### 4. Channels View

**What it shows:**
- Card per channel (webchat, telegram, discord, imessage)
- Status indicator (enabled/disabled based on config)
- Channel-specific metadata (port for webchat, bot token presence for telegram/discord)
- Link to config section for that channel

**API calls:**
- `GET /api/v1/os/channels` — active channel list
- `GET /api/v1/os/config/get` — channel config details

**No new backend work needed.** (Future: per-channel health/connection status
endpoint would improve this — but not blocking.)

### 5. Memory View

**What it shows:**
- Search form: channel_id, sender_id, query text, limit slider (1-50)
- Results list: content, type, created_at, importance score
- Summarize form: channel_id, sender_id, horizon (e.g., "30d", "7d", "1h")
- Summary display (text block)
- Disabled/empty state when memory is off (check config)

**API calls:**
- `POST /api/v1/os/memory/search`
- `POST /api/v1/os/memory/summarize`
- `GET /api/v1/os/config/get` — check memory.enabled

**No new backend work needed.**

### 6. Skills View

**What it shows:**
- Search box + results list (name, description, created_at)
- Install form: name + description fields
- Success/error feedback

**API calls:**
- `GET /api/v1/os/skills/search?q=`
- `POST /api/v1/os/skills/install`

**No new backend work needed.**

### 7. Health View

**What it shows:**
- Health status (green/red dot based on `/health` response)
- Active channels list
- Session count
- Config file path + base_hash
- Runtime mode (dev/prod)
- Auto-refresh on interval

**API calls:**
- `GET /api/v1/os/health`
- `GET /api/v1/os/channels`
- `GET /api/v1/os/sessions` — count only
- `GET /api/v1/os/config/get` — runtime info

**No new backend work needed.**

## API Client (`api.ts`)

Single module with typed functions for every endpoint:

```typescript
// Base
const API_BASE = '/api/v1/os'

// Health
fetchHealth(): Promise<{ status: string }>

// Config
fetchConfig(): Promise<ConfigSnapshot>
patchConfig(baseHash: string, patch: object): Promise<ConfigSnapshot>

// Channels
fetchChannels(): Promise<{ channels: string[] }>

// Sessions
fetchSessions(): Promise<{ sessions: SessionSummary[] }>
deleteSession(id: string): Promise<{ status: string }>

// Messages
sendMessage(channel: string, recipient: string, message: string): Promise<{ status: string }>

// Memory
searchMemory(params: MemorySearchParams): Promise<MemorySearchResult>
summarizeMemory(params: MemorySummarizeParams): Promise<MemorySummarizeResult>

// Skills
searchSkills(query: string): Promise<{ skills: MemoryItem[] }>
installSkill(name: string, description: string): Promise<{ status: string }>
```

Error handling: check `response.status` field, surface errors in UI.

## Types (`types.ts`)

Extract from backend audit:

```typescript
type ConnectionStatus = 'disconnected' | 'connecting' | 'connected'
type RuntimeMode = 'dev' | 'prod'
type ApprovalMode = 'human' | 'ai' | 'auto'
type QueueMode = 'followup' | 'collect' | 'steer' | 'interrupt'

type ConfigSnapshot = {
  status: string
  path: string
  base_hash: string
  updated_at: string
  config: OpenCrawConfig
}

type OpenCrawConfig = {
  general: { model: string; system_prompt: string }
  runtime: { mode: RuntimeMode; data_dir: string }
  keys: { openai_api_key?: string; anthropic_api_key?: string }
  channels: {
    webchat: { enabled: boolean; port: number }
    telegram: { enabled: boolean; bot_token: string }
    discord: { enabled: boolean; bot_token: string }
    imessage: {
      enabled: boolean; source_db?: string; poll_interval_ms: number
      start_from_latest: boolean; group_prefixes: string[]
    }
  }
  tools: { shell: boolean; filesystem: boolean; browser: boolean; clipboard: boolean }
  security: {
    shell_approval: ApprovalMode; browser_approval: ApprovalMode
    filesystem_write_approval: ApprovalMode
    allowed_users: string[]; allow_all_senders: boolean
  }
  queue: { mode: QueueMode; max_concurrency: number; lane_buffer: number }
  context: {
    max_prompt_tokens: number; min_recent_messages: number; max_tool_chars: number
    compaction_enabled: boolean; compaction_trigger_tokens: number
    compaction_retain_messages: number; compaction_horizon: string
    compaction_flush_max_chars: number
  }
  memory: { enabled: boolean }
  optimization: { enabled: boolean; schedule: string }
}

type SessionSummary = {
  id: string; channel_id: string; sender_id: string
  created_at: string; last_active: string; messages: number
}

type ChatItem = {
  id: string; at: number
  role: 'you' | 'assistant' | 'system'
  text: string
}

type MemoryItem = {
  content: unknown; memory_type: string
  created_at: string; importance: number; index_text: string
}
```

## Implementation Sequence

### Phase 1: Extract + Shell

1. Create `types.ts` with shared types
2. Create `api.ts` with all API client functions
3. Extract `StatusChip` and `MessageRow` into `components/`
4. Extract chat logic from `App.tsx` into `views/ChatView.tsx`
5. Build `Sidebar.tsx` with nav items
6. Restructure `App.tsx` as the shell: sidebar + view switcher (state-driven)
7. Wire Chat view as default — app should work identically to today

### Phase 2: Read-Only Views

8. Build `HealthView.tsx` — simplest view, validates API wiring
9. Build `ChannelsView.tsx` — channel cards with enabled/disabled status
10. Build `SessionsView.tsx` — session table with delete action

### Phase 3: Interactive Views

11. Build `ConfigView.tsx` — sectioned form with patch/save
12. Build `MemoryView.tsx` — search + summarize
13. Build `SkillsView.tsx` — search + install

### Phase 4: Polish

14. Sidebar: active state, badge for connection status on Chat item
15. Responsive: sidebar collapse on small screens
16. Loading/error states for all API calls
17. Toast/notification for save success, delete confirm, errors

## Design Notes

- Keep Creek tokens (teal/moss/clay palette, Sora + IBM Plex Mono fonts)
- Sidebar uses `surface` bg, `line` borders, `accent` highlight for active item
- View content area uses `canvas-deep` bg (same as chat log currently)
- Cards within views use `surface-card` class
- All form inputs reuse the existing input styling from the composer
- Status indicators: green dot = active/connected, muted = inactive/disabled

## What This Plan Does NOT Cover

These are future work, not needed for the initial Control UI:

- **Streaming tool output cards** — backend doesn't stream tool events to the UI yet
- **Cron/automation management** — no backend support
- **Exec approvals UI** — approval is handled server-side
- **Node topology** — no node system
- **Log viewer** — no log streaming endpoint
- **Update manager** — no update endpoint
- **Device pairing UI** — pairing is config-driven
- **WebSocket RPC migration** — all views use REST; chat uses WS. A unified WS
  RPC protocol (like OpenClaw's) would be a separate infrastructure project.

## Verification

1. `bun run build` — no TS errors after each phase
2. Each view renders with mock/empty data when backend is unreachable
3. Config patch round-trip: GET config, modify field, PATCH, GET again — value persisted
4. Session delete: session disappears from list after delete
5. Chat still works exactly as before
