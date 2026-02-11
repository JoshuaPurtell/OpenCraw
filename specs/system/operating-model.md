# OpenClaw vs OpenCraw Operating Model (Strict I/O + Read/Write Contracts)

Generated: 2026-02-07

This document defines:
- How OpenClaw works (reference behavior).
- How OpenCraw should work on Horizons (target behavior).
- Exact input/output contracts and read/write boundaries.
- What is correct vs incorrect implementation behavior.

## 1. OpenClaw Runtime Model (Reference)

OpenClaw is a single gateway runtime with strict config and a message-driven agent loop.

Core flow:
1. Channel/event ingress arrives (Telegram/Discord/WhatsApp/etc).
2. Gateway validates config/session/policy.
3. Message is enqueued into a lane (per user/thread semantics).
4. Agent run executes with tools, memory, and policy gates.
5. Outbound replies/tool side effects are emitted.
6. Transcript, memory, and state are persisted.

Properties:
- Strong config schema validation at startup.
- Lane-aware queueing and session serialization.
- Strict policy controls for DM/group/tool behavior.
- Persistent state and memory as first-class behavior.

## 2. OpenCraw Target Model on Horizons (Correct Architecture)

OpenCraw should be an opinionated application layer over Horizons primitives.

Layer model:
1. Ingress layer (`os-channels`)
2. Orchestration layer (`os-app::gateway` + lane queue)
3. Agent layer (`os-app::assistant`)
4. Capability layer (`os-tools` + channel send paths)
5. Platform layer (Horizons memory/eval/optimization/audit)
6. Egress layer (`os-channels` outbound)

### Correct responsibility split
- OpenCraw owns channel normalization, UX behavior, and product policy.
- Horizons owns durable agent infrastructure: memory/evaluation/optimization/governance.
- Tools perform explicit side effects only when invoked.

## 3. Input/Output Contract (System-Level)

## Input types
- External user messages from channels (Telegram, iMessage, email, Linear, webchat).
- Channel reactions/events.
- HTTP API calls (health/config/messages/sessions).
- Scheduled or webhook triggers (future).

## Output types
- Channel replies/messages (text first, attachments later).
- Tool side effects (filesystem write, shell exec, email send, etc).
- Persisted session state and memory records.
- Structured logs/audit entries.

## Non-negotiable correctness
- No implicit fallback behavior on invalid config or malformed tool payloads.
- Every side effect is attributable (tool name, arguments, actor/session, outcome).
- Every ingress event either:
  - is processed and acknowledged,
  - is rejected with explicit reason,
  - or fails with explicit error.

## 4. Read/Write Matrix (What Reads/Writes What)

## Channel adapters (`os-channels`)
- Reads: channel APIs/SDKs (Telegram Bot API, Gmail API, etc).
- Writes: outbound channel APIs only.
- Must NOT: decide policy, mutate business state, or execute arbitrary tools.

## Gateway (`os-app::gateway`)
- Reads: inbound queue, config, allowlist policy, session state.
- Writes: lane queues, session updates, outbound channel sends.
- Must NOT: invent fallback behavior for invalid channel payloads.

## Assistant (`os-app::assistant`)
- Reads: system prompt, session context, memory retrieval, tool specs.
- Writes: assistant responses, tool call records, memory append/summary.
- Must NOT: bypass approval/risk gates for high-risk actions.

## Tools (`os-tools`)
- Reads: explicit arguments + external system state needed for action.
- Writes: only the declared side effect for the invoked action.
- Must: validate arguments strictly and fail with actionable errors.
- Must NOT: perform hidden extra writes.

## Horizons platform components
- Reads/Writes: project DB, memory stores, eval artifacts, optimization artifacts, events.
- Must provide deterministic governance/audit semantics to OpenCraw.

## 5. Correct Interaction Pattern for Email via Telegram

Use Telegram as control plane, Email as target capability.

Flow:
1. Telegram message arrives.
2. Assistant decides to call `email` tool action (e.g., `search`, `read`, `send`).
3. Tool executes Gmail API call.
4. Tool returns structured result.
5. Assistant summarizes result back to Telegram.

Correctness rules:
- `send` is high-risk and approval-gated (human/AI/auto per policy).
- Tool args are strict (missing/invalid fields fail immediately).
- No hidden retries that mask semantic errors.
- Channel adapters remain transport-only.

## 6. Telegram Constraint Clarification

Telegram bots cannot proactively DM arbitrary users who have never initiated chat with the bot.

Therefore:
- Bot can reply in existing chats (DM/group) where it has access.
- Bot cannot originate a new private chat to a user who never started it.

Correct product behavior:
- Fail explicitly when attempted target is unreachable under Bot API constraints.
- Provide actionable reason in error/log.

## 7. Strict Failure Semantics (Fail Fast)

Startup must fail on:
- invalid config schema or unsupported enum values,
- missing required credentials when channel/tool is enabled,
- invalid tool names violating provider constraints,
- bind failures / required runtime wiring failures.

Run-time must fail on:
- malformed tool arguments,
- unknown tool actions,
- unknown referenced tools,
- external API non-success status (with status + body surfaced).

Disallowed patterns:
- silent fallback to alternate provider/model/tool,
- silently skipping invalid config keys,
- implicit downgrade of side-effecting operations.

## 8. What OpenCraw Should Implement Next (Feature-Forward, Architecture-Correct)

1. Email tool integration in `os-tools` and tool wiring in `os-app`.
2. Linear tool integration with same strict action schema and risk policy.
3. Session persistence + compaction defaults (Horizons-backed).
4. Skill/profile system for tool allow/deny by channel/user.
5. Rich channel capability matrix (Slack/WhatsApp/Signal/Matrix).

This sequence preserves strictness while adding OpenClaw-level utility.

## 9. Practical Definition of “Correct” for OpenCraw

A behavior is correct if it is:
- Explicit: all side effects are intentional and named.
- Deterministic: same input/config yields same control flow.
- Observable: logs explain decisions and failures.
- Governed: risky actions pass through approval policy.
- Strict: invalid input/config fails loudly, never silently patched.
