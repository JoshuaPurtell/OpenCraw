# OpenCraw Planning Artifacts

Generated: 2026-02-07

This folder contains the full discovery and planning output requested for:

- `/Users/synth/Desktop/opencraw-plan.md`
- `/Users/synth/OpenCraw`
- `/Users/synth/horizons`
- Official OpenClaw docs (including `llms.txt` / `llms-full.txt`)

## Documents

1. `01-system-audit.md`
   - Verified current state of OpenCraw and Horizons plus OpenClaw reference capabilities.
2. `02-parity-gap-matrix.md`
   - Capability-by-capability gap analysis and priority scoring.
3. `03-target-architecture.md`
   - Target architecture for building a full OpenClaw-equivalent on Horizons.
4. `04-implementation-roadmap.md`
   - Sequenced rollout plan with milestones, acceptance criteria, and execution log updates.
5. `05-work-backlog.md`
   - Actionable epics/tasks with dependencies, implementation notes, and checked progress items.
6. `06-source-log.md`
   - Evidence map of local repositories, docs URLs, and validation commands used.
7. `07-openclaw-style-guide.md`
   - UX and interaction style decisions for the OpenCraw web surface toward OpenClaw parity.
8. `08-opencraw-creek-visual-redesign-plan.md`
   - Visual design and frontend implementation plan for the Creek-themed experience.
9. `09-channel-setup-imessage-telegram.md`
   - Exact production setup and verification steps for iMessage and Telegram channels.
10. `10-channel-setup-email-linear.md`
   - Exact production setup and verification steps for Email (Gmail) and Linear channels.
11. `11-email-usage-guide.md`
   - Operational usage guide for Email (Gmail): inbound behavior, send patterns, allowlist, and failure modes.
12. `12-linear-usage-guide.md`
   - Operational usage guide for Linear: inbound behavior, team filters, send patterns, and failure modes.
13. `13-webchat-usage-guide.md`
   - Operational usage guide for WebChat: session behavior, transport details, and failure modes.
14. `14-imessage-usage-guide.md`
   - Operational usage guide for iMessage: sender mapping, group mention behavior, and failure modes.
15. `15-telegram-usage-guide.md`
   - Operational usage guide for Telegram: update handling, reactions, and delivery constraints.
16. `16-opencraw-openclaw-operating-model.md`
   - Canonical system model for OpenClaw vs OpenCraw with strict input/output and read/write contracts.

## Integration Note

OpenClaw capability deltas from `/Users/synth/Desktop/openclaw-features.md` are incorporated directly into:

- `01-system-audit.md`
- `02-parity-gap-matrix.md`
- `03-target-architecture.md`
- `04-implementation-roadmap.md`
- `05-work-backlog.md`

Fail-fast strictness implementation progress is tracked directly in:

- `04-implementation-roadmap.md` (execution log)
- `05-work-backlog.md` (progress notes + DoD updates)
- `06-source-log.md` (verification evidence)

Channel onboarding runbooks are now tracked in:

- `09-channel-setup-imessage-telegram.md`
- `10-channel-setup-email-linear.md`

Channel operational usage guides are now tracked in:

- `11-email-usage-guide.md`
- `12-linear-usage-guide.md`
