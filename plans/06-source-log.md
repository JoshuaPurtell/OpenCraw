# Source Log and Evidence Map

Generated: 2026-02-07

This file records the primary evidence used to build the planning docs.

## Local Plan Input

- `/Users/synth/Desktop/opencraw-plan.md`
- `/Users/synth/Desktop/openclaw-features.md` (updated feature inventory used to deepen parity requirements)

## OpenCraw Code Evidence

- Runtime wiring: `/Users/synth/OpenCraw/os-app/src/server.rs`
- Tracing/bootstrap wiring: `/Users/synth/OpenCraw/os-app/src/main.rs`
- Runtime backend composition (dev + prod): `/Users/synth/OpenCraw/os-app/src/dev_backends.rs`
- Gateway loop and message handling: `/Users/synth/OpenCraw/os-app/src/gateway.rs`
- Assistant loop and approvals: `/Users/synth/OpenCraw/os-app/src/assistant.rs`
  - Includes assistant-facing Horizons memory tool wrappers (`memory.search`, `memory.summarize`) with scoped retrieval/summarization.
  - Includes Horizons-backed pre-compaction memory flush + summary checkpoint rewrite flow.
- Session storage (ProjectDb-backed): `/Users/synth/OpenCraw/os-app/src/session.rs`
- Config and model detection: `/Users/synth/OpenCraw/os-app/src/config.rs`
  - Includes strict context compaction config validation (`context.compaction_enabled` requires `memory.enabled`).
- OpenCraw memory API routes: `/Users/synth/OpenCraw/os-app/src/routes/memory.rs`
- Tool implementations: `/Users/synth/OpenCraw/os-tools/src/*.rs`
- Channel implementations: `/Users/synth/OpenCraw/os-channels/src/*.rs`
- LLM provider handling: `/Users/synth/OpenCraw/os-llm/src/client.rs`
- Strictness/fail-fast hardening touchpoints:
  - `/Users/synth/OpenCraw/os-app/src/config.rs`
  - `/Users/synth/OpenCraw/os-app/src/gateway.rs`
  - `/Users/synth/OpenCraw/os-app/src/assistant.rs`
  - `/Users/synth/OpenCraw/os-channels/src/{telegram.rs,discord.rs,webchat.rs,imessage.rs,email.rs,linear.rs}`
  - `/Users/synth/OpenCraw/os-llm/src/{client.rs,openai.rs,anthropic.rs}`

## Horizons Code Evidence

- API router surface: `/Users/synth/horizons/horizons_server/src/routes/mod.rs`
- Memory routes: `/Users/synth/horizons/horizons_server/src/routes/memory.rs`
- Optimization routes: `/Users/synth/horizons/horizons_server/src/routes/optimization.rs`
- Evaluation routes: `/Users/synth/horizons/horizons_server/src/routes/evaluation.rs`
- Graph routes: `/Users/synth/horizons/horizons_server/src/routes/graph.rs`
- Pipeline routes: `/Users/synth/horizons/horizons_server/src/routes/pipelines.rs`
- Context refresh routes: `/Users/synth/horizons/horizons_server/src/routes/context_refresh.rs`
- Engine routes: `/Users/synth/horizons/horizons_server/src/routes/engine.rs`
- MCP routes: `/Users/synth/horizons/horizons_server/src/routes/mcp.rs`
- Core agent scheduler: `/Users/synth/horizons/horizons_core/src/core_agents/scheduler.rs`
- Sandbox runtime and backends:
  - `/Users/synth/horizons/horizons_core/src/engine/sandbox_runtime.rs`
  - `/Users/synth/horizons/horizons_core/src/engine/docker_backend.rs`
  - `/Users/synth/horizons/horizons_core/src/engine/daytona_backend.rs`
- Platform issue list: `/Users/synth/horizons/issues.txt`

## OpenClaw Official Docs Evidence

- Docs index and LLM doc endpoints:
  - `https://docs.openclaw.ai/llms.txt`
  - `https://docs.openclaw.ai/llms-full.txt`
- Architecture and protocol behavior:
  - `https://docs.openclaw.ai/concepts/architecture`
- Queue semantics:
  - `https://docs.openclaw.ai/concepts/queue`
- Memory model:
  - `https://docs.openclaw.ai/concepts/memory`
- Session model:
  - `https://docs.openclaw.ai/concepts/session`
- Model failover:
  - `https://docs.openclaw.ai/concepts/model-failover`
- Channels matrix:
  - `https://docs.openclaw.ai/channels`
- Feature overview:
  - `https://docs.openclaw.ai/concepts/features`
- Tool profiles and configuration:
  - `https://docs.openclaw.ai/gateway/configuration`
- Providers:
  - `https://docs.openclaw.ai/concepts/model-providers`
- Multi-agent behavior:
  - `https://docs.openclaw.ai/concepts/multi-agent`
- Streaming/chunking:
  - `https://docs.openclaw.ai/concepts/streaming`
- Cron automation behavior:
  - `https://docs.openclaw.ai/automation/cron-jobs`

## Verification Commands Run

- `cargo check` in `/Users/synth/OpenCraw`
  - Result: success after strict fail-fast hardening changes
- `cargo test` in `/Users/synth/OpenCraw`
  - Result: success (all package/unit/doc tests passed)
- `cargo test -p os-app` in `/Users/synth/OpenCraw`
  - Result: success (7 tests passed), including config patch/hash and session persistence reload tests
- `cargo test -p os-llm` in `/Users/synth/OpenCraw`
  - Result: success (provider/tool sanitization unit tests passed)
- `cargo test -p os-llm -p os-app` in `/Users/synth/OpenCraw`
  - Result: success (all `os-llm` and `os-app` unit/doc tests passed after observability hardening)
- `cargo check -p os-app` in `/Users/synth/OpenCraw`
  - Result: success after production runtime wiring and ProjectDb session-store migration
- `cargo test -p os-app` in `/Users/synth/OpenCraw`
  - Result: success (19 tests passed), including queue-mode reshaping/interrupt-signal tests, assistant memory-tool + compaction-helper tests, memory-route validation tests, and ProjectDb-backed session persistence tests
- `cargo fmt` in `/Users/synth/OpenCraw`
  - Result: success (formatting clean after queue-mode implementation changes)
- `cargo run -p os-app -- doctor --config /Users/synth/OpenCraw/config.example.toml`
  - Result: fails fast as expected when required provider key is missing (`keys.anthropic_api_key is required for claude models`)
- `ANTHROPIC_API_KEY=test-key cargo run -p os-app -- doctor --config /Users/synth/OpenCraw/config.example.toml`
  - Result: success (doctor path validates and exits cleanly once required provider key is supplied)
- `OPENSHELL_QUEUE_MODE=interrupt ANTHROPIC_API_KEY=test-key cargo run -p os-app -- doctor --config /Users/synth/OpenCraw/config.example.toml`
  - Result: success (queue mode override accepted in strict parsing path)
- `OPENSHELL_QUEUE_MODE=not-a-mode ANTHROPIC_API_KEY=test-key cargo run -p os-app -- doctor --config /Users/synth/OpenCraw/config.example.toml`
  - Result: fails fast as expected (`invalid OPENSHELL_QUEUE_MODE="not-a-mode"`)
- `OPENSHELL_CONTEXT_COMPACTION_ENABLED=true ANTHROPIC_API_KEY=test-key cargo run -p os-app -- doctor --config /Users/synth/OpenCraw/config.example.toml`
  - Result: fails fast as expected (`context.compaction_enabled=true requires memory.enabled=true`)
- `OPENSHELL_RUNTIME_MODE=prod ANTHROPIC_API_KEY=test-key cargo run -p os-app -- serve --config /Users/synth/OpenCraw/config.example.toml`
  - Result: fails fast as expected when required production runtime variable is missing (`OPENCRAW_ORG_ID`)
- `grep -n '^## ' /Users/synth/Desktop/openclaw-features.md`
  - Result: section inventory extracted for architecture/channel/tool/security/model/automation/ops coverage checks
- `grep -n '^### ' /Users/synth/Desktop/openclaw-features.md`
  - Result: subsection inventory extracted to cross-check detail-level gaps in planning docs
- `cargo check -p os-channels -p os-app` in `/Users/synth/OpenCraw`
  - Result: success after adding Email (Gmail) and Linear channel adapters
- `cargo test -p os-channels` in `/Users/synth/OpenCraw`
  - Result: failed due machine disk exhaustion (`No space left on device`) while compiling dependencies; no adapter-level test failure signal was emitted before storage error
