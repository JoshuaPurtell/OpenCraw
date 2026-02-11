# Horizons API Master Contract

Last updated (UTC): 2026-02-10T23:17:42Z  
Horizons source snapshot: `~/horizons` (`main` @ `2018c90`)  
Status: Canonical reference for all Zangbot planning docs

## Purpose

This document is the single source of truth for what Horizons actually exposes and enforces today.

Use this document to avoid drift between Zangbot planning assumptions and Horizons runtime behavior.

## Regeneration Trigger

Regenerate this file whenever either condition is true:

1. `git -C ~/horizons rev-parse HEAD` changes.
2. Any of these files change in Horizons:
   - `horizons_server/src/routes/**`
   - `horizons_server/src/extract.rs`
   - `horizons_server/src/middleware.rs`
   - `horizons_core/src/events/**`
   - `horizons_core/src/core_agents/**`
   - `horizons_core/src/onboard/traits.rs`

## Authoritative Source Files

- Route wiring and API surface:
  - `~/horizons/horizons_server/src/routes/mod.rs`
  - `~/horizons/horizons_server/src/server.rs`
- Auth and header extraction:
  - `~/horizons/horizons_server/src/extract.rs`
  - `~/horizons/horizons_server/src/middleware.rs`
- Core data plane contracts:
  - `~/horizons/horizons_core/src/onboard/traits.rs`
- Event contracts and persistence:
  - `~/horizons/horizons_core/src/events/models.rs`
  - `~/horizons/horizons_core/src/events/store.rs`
  - `~/horizons/horizons_core/src/events/bus.rs`
- Action contracts and persistence:
  - `~/horizons/horizons_core/src/core_agents/models.rs`
  - `~/horizons/horizons_core/src/core_agents/approvals.rs`
  - `~/horizons/horizons_core/src/core_agents/executor.rs`

## Planes and Storage Boundaries

Horizons has four practical planes relevant to Zangbot:

1. Platform plane (CentralDb): org/user/project slug metadata, API keys, platform config, audit, credentials, connector/source config, refresh runs, resources/operations, core-agent registry.
2. Domain plane (ProjectDb): per-project database handles and raw SQL execution for app-owned schema.
3. Event plane (EventBus/EventStore): durable event publish/query/subscriptions with delivery status and retry/DLQ behavior.
4. File plane (Filestore): org/project-scoped object storage via key paths.

## Auth, Identity, and Scope Contract

## Auth modes

Auth behavior is environment-configurable (`HORIZONS_AUTH_MODE` and related flags), with defaults equivalent to:

- `require_auth = false`
- `allow_insecure_headers = true`
- `require_auth_for_mutating = true`
- `allow_insecure_mutating_requests = false`

## Mutating route guard

- Mutating HTTP verbs on `/api/v1/*` are blocked unless verified auth succeeds.
- Exception path: dev-only insecure override can allow header-based mutating requests.
- `/events/inbound` is intentionally outside `/api/v1` and is not protected by the mutating middleware.

## Identity extraction

Resolution order:

1. Auth provider (Bearer/API-key path) if configured.
2. Fallback headers:
   - `x-org-id` for tenant
   - `x-user-id` + optional `x-user-email`, or `x-agent-id` for actor
   - If no actor headers, defaults to `system:http`.

## Project scope extraction

- `x-project-id` is optional and parsed when present.
- Endpoints that require project scope enforce it explicitly.

## API Surface Inventory

All endpoints below are under `/api/v1` unless explicitly marked otherwise.

## System and non-versioned

- `GET /health`
- `GET /api/v1/health`
- `POST /events/inbound` (public ingest endpoint)
- `GET /admin`
- `GET /admin/`
- `GET /admin/agents`
- `GET /admin/graphs`
- `GET /admin/backends`

## Org and project provisioning

- `POST /orgs`
- `POST /projects`
- `GET /projects`
- `POST /projects/{id}/query`
- `POST /projects/{id}/execute`

## Files

- `PUT /files/{*key}`
- `GET /files/{*key}`
- `DELETE /files/{*key}`

## Events and subscriptions

- `GET /events`
- `POST /events/publish`
- `POST /subscriptions`
- `GET /subscriptions`
- `DELETE /subscriptions/{id}`

## Actions and approvals

- `POST /actions/propose`
- `POST /actions/{id}/approve`
- `POST /actions/{id}/deny`
- `GET /actions/pending`

## Connectors and context refresh

- `POST /connectors`
- `GET /connectors`
- `POST /context-refresh/run`
- `GET /context-refresh/status`

## Pipelines

- `POST /pipelines/run`
- `GET /pipelines/runs/{id}`
- `POST /pipelines/runs/{id}/approve/{step_id}`
- `POST /pipelines/runs/{id}/cancel`

## MCP

- `POST /mcp/config`
- `GET /mcp/tools`
- `POST /mcp/call`

## Agents and core agents

- `POST /agents/run`
- `POST /agents/chat`
- `GET /agents`
- `GET /core_agents`
- `PUT /core_agents`
- `GET /core_agents/{agent_id}`
- `DELETE /core_agents/{agent_id}`
- `POST /core_agents/time_enabled`

## Engine

- `POST /engine/run`
- `POST /engine/start`
- `GET /engine/{handle_id}/events`
- `POST /engine/{handle_id}/message`
- `POST /engine/{handle_id}/release`
- `GET /engine/{handle_id}/health`

## Graph

- `POST /graph/query`
- `POST /graph/upsert`
- `POST /graph/validate`
- `POST /graph/normalize`
- `POST /graph/execute`
- `GET /graph/registry`
- `GET /graph/registry/{graph_id}`

## Audit, assets, credentials, config, scheduler tick

- `GET /audit`
- `POST /assets/resources`
- `GET /assets/resources`
- `GET /assets/resources/{id}`
- `POST /assets/operations`
- `GET /assets/operations`
- `GET /assets/operations/{id}`
- `GET /assets/operations/runs`
- `GET /credentials`
- `GET /credentials/{connector_id}`
- `PUT /credentials/{connector_id}`
- `DELETE /credentials/{connector_id}`
- `GET /config`
- `PUT /config`
- `POST /tick`

## Feature-gated endpoints

When compiled with corresponding feature flags:

- Memory:
  - `GET /memory`
  - `POST /memory`
  - `POST /memory/summarize`
- Optimization:
  - `POST /optimization/run`
  - `GET /optimization/status`
  - `GET /optimization/reports`
  - `POST /optimization/cycles`
- Evaluation:
  - `POST /eval/run`
  - `GET /eval/reports`

## Canonical Event Contract

## Event model fields

Durable event rows contain:

- `id`
- `org_id`
- optional `project_id`
- `timestamp`
- `received_at`
- `direction`
- `topic`
- `source`
- `payload`
- `dedupe_key`
- `status`
- `retry_count`
- `metadata`
- optional `last_attempt_at`

## Event dedupe and uniqueness

Event dedupe is enforced by a unique index on:

- (`org_id`, `dedupe_key`)

There is no built-in `project_id` component in event dedupe uniqueness.

## Event lifecycle mutability

Event rows are append-only for identity/payload but mutable for delivery process fields:

- `status` is updated in place.
- `retry_count` is incremented in place.
- `last_attempt_at` is updated in place.

## Event query contract

- Queries are org-scoped.
- Optional filters: project, topic, direction, status, since/until.
- Ordered by `received_at ASC`.
- `limit > 0` is required.

## Canonical Action Contract

## Action model fields

`ActionProposal` includes:

- `id`
- `org_id`
- `project_id`
- `agent_id`
- `action_type`
- `payload`
- `risk_level`
- optional `dedupe_key`
- `context`
- `status`
- `created_at`
- optional `decided_at`
- optional `decided_by`
- optional `decision_reason`
- `expires_at`
- optional `execution_result`

## Action statuses and transitions

Statuses:

- `proposed`
- `approved`
- `denied`
- `dispatched` (`executed` accepted as legacy alias)
- `expired`

Transitions are validated by approval state-machine logic before persistence.

## Action persistence shape

Horizons currently stores actions as one mutable row in `horizons_action_proposals` (project DB), not as immutable proposal/decision/execution triplet rows.

In-place updates include:

- `status`
- `decided_at`
- `decided_by`
- `decision_reason`
- `execution_result_json`

## Action dedupe and uniqueness

Action dedupe is enforced by unique index:

- (`org_id`, `dedupe_key`)

There is no built-in `project_id` component in action dedupe uniqueness.

## Pending actions storage location

- Pending actions are queried from project DB table `horizons_action_proposals`.
- Query filter includes `org_id`, `project_id`, `status='proposed'`.

## Project DB Raw SQL Contract

`/projects/{id}/query`:

- Read-only SQL only.
- Multi-statement payloads are rejected.
- Result row count is capped (`HORIZONS_PROJECTDB_MAX_ROWS`, default 1000).
- Timeout enforced (`HORIZONS_PROJECTDB_TIMEOUT_MS`, default 10000 ms).

`/projects/{id}/execute`:

- Write SQL only.
- Disabled unless `HORIZONS_PROJECTDB_ALLOW_WRITES=true`.
- Timeout enforced (`HORIZONS_PROJECTDB_TIMEOUT_MS`).

## File Contract

File operations are scoped by:

- required org
- optional project
- opaque key path

Routes:

- `PUT /files/{*key}`
- `GET /files/{*key}`
- `DELETE /files/{*key}`

## Integration-Oriented Contracts

## Context refresh and connectors

Source registration and refresh contracts are first-class:

- `SourceConfig` includes org/project, connector id, scope, schedule, event triggers, settings, processor spec.
- `RefreshRun` tracks trigger, status, counts, cursor, and error details.

## Pipelines

Pipelines are first-class:

- `PipelineSpec` with ordered steps, dependencies, retries, optional step-level approvals.
- `PipelineRun` with run status and per-step results.

## MCP gateway

MCP is first-class:

- Runtime-configurable server list.
- Tool listing and invocation over stdio or HTTP transports.
- Scope enforcement is policy-driven, not caller-selected.

## Zangbot Binding Rules (Mandatory)

1. Treat this file as authoritative for Horizons transport/storage contracts.
2. Do not assume Horizons event top-level fields include `legal_entity_id` or `operating_unit_id`; carry financial scope in Zangbot domain records and/or event payload metadata.
3. Do not assume project-scoped dedupe in Horizons actions/events; generate dedupe keys that are org-unique (or include project and domain identifiers within the key string).
4. Do not assume immutable action triplet storage in Horizons core action table; if Zangbot requires proposal/decision/execution triplets, model and persist them in Zangbot domain storage explicitly.

## Change Control

When regenerating this document:

1. Update `Last updated (UTC)` and snapshot commit metadata.
2. Re-verify route map from `horizons_server/src/routes/mod.rs`.
3. Re-verify event/action dedupe and mutability constraints from `events/store.rs` and `core_agents/executor.rs`.
4. Reconcile dependent planning docs (`05-data-structure.md`) to remove stale or conflicting assumptions.
