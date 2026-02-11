# Verification Governance Standard

Date: 2026-02-11
Status: active
Owner: platform

## Goal

Define a verification model where documentation contracts are authoritative and enforcement is independent of any single CI workflow or shell script.

## 1) Authority Model

1. `AGENTS.md`, `standards/*`, and accepted `specs/foundation/*` are the source of truth.
2. CI workflows, shell scripts, and local wrappers are execution helpers, not policy authority.
3. Verification requirements remain mandatory even when a specific runner is unavailable.

## 2) Accepted Verification Runners

A verification run is acceptable when it can prove command, environment, and result through durable evidence. Approved runners:

1. CI workflow execution.
2. Local maintainer execution.
3. Parallel certification lane execution.
4. Future runners that preserve equivalent evidence quality.

## 3) Evidence Minimum

Every non-trivial change MUST include:

1. Change classification (`foundation-hardening`, `feature-delivery`, `maintenance`).
2. Exact commands executed.
3. Runner identity and environment snapshot (OS/toolchain context).
4. Pass/fail outcome per command.
5. Durable artifact location (PR body, job URL, or committed evidence file).
6. Any skipped checks with explicit reason.

## 4) Failure Semantics

1. Broken CI or script automation does not waive required checks.
2. If one runner fails operationally, execute equivalent gates in another approved runner.
3. "Green by omission" is invalid evidence.

## 5) Rust-Specific Requirement

For Rust code changes, required commands are defined by:

1. `AGENTS.md` mandatory quality gate.
2. `standards/foundation-gates.md` required quality gates.

## 6) Waiver Governance

A waiver is valid only if all are documented:

1. Owner.
2. Scope.
3. Expiry date.
4. Compensating control.
5. Rollback plan.
6. Approval record linked to the change.

Waivers are temporary and MUST be removed when expiry is reached.

## 7) Multi-Agent Verification Rule

1. Each lane owns evidence for its allowlisted changes.
2. Integration lane confirms cross-lane evidence completeness before merge.
3. Certification lane provides tier-promotion evidence as artifacts, not assertions.
