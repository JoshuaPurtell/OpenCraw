# OpenCraw vs OpenClaw Parity Gap Matrix

Generated: 2026-02-07

Legend:

- Status: `Done`, `Partial`, `Missing`
- Priority: `P0` (must-have parity blocker), `P1` (high), `P2` (medium), `P3` (later)
- Leverage: Horizons capability that can close the gap faster

| Area | OpenClaw Baseline | OpenCraw Today | Horizons Leverage | Gap | Priority |
|---|---|---|---|---|---|
| Gateway topology | Single long-lived WS gateway with typed protocol | Basic multi-channel gateway loop | CoreAgents + events + pipelines | Partial | P1 |
| Gateway config control plane | `config.get`/`config.apply`/`config.patch` (`baseHash`) semantics | Static local config load only | Core config service + eventing | Missing | P0 |
| Bind/auth/discovery matrix | `loopback`/`lan`/`tailnet`/`auto`/`custom`, discovery modes, auth coupling | No equivalent mode matrix | Gateway policy + identity services | Missing | P0 |
| Queue lanes/modes | Lane-aware queue with steer/followup/collect/interrupt | Single global inbound loop | Event bus + pipeline scheduler | Missing | P0 |
| Debounce and backlog | Per-sender/channel debounce and backlog controls | None | Event bus + queue abstraction | Missing | P0 |
| Queue drop policies | Keep/drop/summarize variants with clear send-policy behavior | None | Queue abstraction + summarizer | Missing | P1 |
| Overflow summarization | Summarize dropped backlog | None | RLM + graph summarizer | Missing | P1 |
| Streaming blocks | Chunked streaming with typing indicators | Non-streaming final response only | Existing `chat_stream` path in `os-llm` | Missing | P0 |
| Chunk/coalesce policy knobs | Per-channel chunking/coalescing and typing behavior | Not configurable | Adapter policy layer | Missing | P1 |
| Sessions durability | Scoped sessions persisted across restarts | In-memory DashMap only | ProjectDb + pipelines | Missing | P0 |
| Session scope model | DM/group/forum/topic/cron/webhook scope model | `(channel_id, sender_id)` only | Context model + identity map | Missing | P0 |
| Session reset policies | Daily + idle reset policies | Manual `/new` only | Scheduler + config policies | Missing | P1 |
| Session tool surface | `sessions_list`/`history`/`send`/`spawn` operations | Only `/new` reset behavior | Session service + queue APIs | Missing | P1 |
| Pruning/compaction | Configurable context pruning/compaction | None | Graph/pipeline summarization | Missing | P0 |
| Pre-compaction memory flush | Silent persistence before trim | None | Voyager memory + pipeline hooks | Missing | P1 |
| Memory architecture | Daily logs + MEMORY.md + hybrid retrieval | Optional Voyager append/retrieve only | Voyager + project DB abstractions | Partial | P0 |
| Memory tools | `memory_search`, `memory_get` semantics | No dedicated memory tools | Memory routes already exist in Horizons | Missing | P1 |
| Bootstrap/prompt assembly | Multi-file bootstrap (`AGENTS/SOUL/TOOLS/...`) and prompt modes | Minimal prompt assembly only | File loader + policy hooks | Missing | P0 |
| Hook lifecycle matrix | `before/after tool`, compaction, gateway lifecycle hooks | Limited/no formal hook matrix | Pipelines + event hooks | Missing | P1 |
| Channel coverage | Broad channel matrix (WhatsApp/Slack/Signal/etc.) | Webchat, Telegram, Discord, iMessage | Existing adapter trait pattern | Partial | P0 |
| Channel policy controls | DM/group policy matrix and per-channel behavior | Allowlist + mention/prefix only | Policy layer via CoreAgents | Partial | P1 |
| Channel operational compatibility | Channel-specific retries/limits/feature knobs | Adapter defaults only | Per-adapter capability schema | Missing | P1 |
| Reactions + feedback loop | Rich reaction handling | Basic positive/negative eval mapping | Evaluation engine already wired | Partial | P2 |
| Tool profiles | Minimal/coding/messaging/full profiles | Boolean on/off by tool | Config + policy system | Missing | P0 |
| Tool groups and filters | Grouped capabilities + allow/deny | No groups, no deny precedence | CoreAgents policy tables | Missing | P1 |
| Web fetch/search tools | Search + readable fetch pipeline | Missing | Could bind via MCP or direct tools | Missing | P0 |
| Background process tools | Async process mgmt and notifications | Foreground `shell.execute` only | Engine + event bus | Missing | P1 |
| Gateway management tools | Restart/config/control actions exposed as tools/commands | None | Gateway admin service | Missing | P1 |
| Media tools | Image/audio/video understanding | Missing | Graph/connector integrations | Missing | P2 |
| Skills loading | Folder hierarchy + hot-reload + constraints | Skill metadata stored in memory only | File watcher + capability checks | Missing | P0 |
| Skills metadata contract | Frontmatter + runtime requirement checks (`bins/env/config`) | No metadata enforcement | Skill loader policy checks | Missing | P1 |
| Model provider matrix | Multi-provider support + aliases | Anthropic + OpenAI-compatible | Horizons is provider-agnostic at platform layer | Partial | P1 |
| Model failover chains | Ordered fallback strategy | None | Config + runtime retry policy | Missing | P0 |
| Two-stage auth/model failover | Rotate auth profile first, then model fallback with cooldowns | None | Auth profile registry + retry engine | Missing | P0 |
| Agent overrides | Per-session/per-agent model and reasoning | Global model in config | Session model fields + policy | Missing | P1 |
| Pairing tiers | Pairing/allowlist/open/disabled modes | Allowlist and allow-all flag | Existing pairing module extension | Partial | P1 |
| Sandbox security model | Strong gateway/tool sandbox policy | No OpenCraw sandbox tool execution | Horizons engine endpoints available | Missing | P1 |
| Security audit and incident ops | Built-in audit modes and incident-response flow | No parity command/workflow | CoreAgents audit + events | Missing | P0 |
| Plugin trust policy | Explicit plugin allow/deny/provenance policy | None | Policy tables + signed metadata | Missing | P1 |
| Multi-agent routing | Binding rules + sub-agents + handoff | Not implemented in OpenCraw | Horizons pipelines + core agents | Missing | P2 |
| Automations | Cron/poll/webhook integrations | Not implemented in OpenCraw app layer | Horizons context refresh + events + scheduler | Missing | P1 |
| Heartbeat contract | `HEARTBEAT_OK` and monitor hierarchy | No heartbeat contract model | Scheduler + automation engine | Missing | P1 |
| Webhooks | Inbound automation hooks | Not implemented in OpenCraw | Horizons events inbound exists | Missing | P1 |
| CLI/slash command surface | Full command-group matrix + registration behavior | `/new` plus ad-hoc controls | Command router + config APIs | Missing | P1 |
| Voice/TTS support | Provider-backed TTS/talk mode with interrupt rules | None | Optional provider abstraction | Missing | P2 |
| Nodes/device capabilities | Device discovery, permissions, capability matrix | None | Companion/node services (future) | Missing | P2 |
| Env/config compatibility inventory | Broad env var and config-key parity | Limited config examples | Config schema + docs generation | Missing | P1 |
| Observability | Operational health and traces | Basic logs only | Horizons OTEL/Langfuse stack | Missing | P2 |
| Auditability | Strong action audit and approvals | Tool approvals exist; app-level trail limited | CoreAgents + audit routes | Partial | P2 |

## Prioritization Summary

## P0 (parity blockers)

- Queue and concurrency model
- Session persistence + scope model
- Context pruning/compaction pipeline
- Streaming responses with typing/chunking
- Gateway config control plane and bind/auth/discovery compatibility
- Tool profile system
- Core web tools and model failover
- Two-stage auth/model failover and cooldown behavior
- Skills system parity
- Bootstrap/prompt assembly and core hook lifecycle parity
- Channel expansion (at least WhatsApp/Slack/Signal/Matrix baseline)
- Security audit/incident-response parity for release claims

## P1 (high-value next)

- DM pairing modes and richer safety tiers
- Background process execution model
- Automation surfaces (cron/webhook/poll)
- Heartbeat contracts and auth monitor flows
- Channel policy refinements
- Channel-specific operational compatibility knobs
- Memory tooling and pre-compaction flush
- Session/gateway admin tools and command-surface compatibility
- Plugin trust policy and env/config matrix compatibility

## P2/P3 (after parity)

- Multi-agent routing depth
- Media-rich tooling
- Full observability productization
- Marketplace/distribution concepts
