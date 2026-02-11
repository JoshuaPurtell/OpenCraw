# Agent Guardrails Contract

- Status: accepted
- Owner: platform
- Last-Reviewed: 2026-02-11
- Scope: all coding agents operating in this repository
- Depends-On: `AGENTS.md`, `standards/rust.md`, `standards/foundation-gates.md`, `standards/verification-governance.md`, `specs/foundation/verification-evidence-contract.md`

## 1. Core Execution Contract

1. Agents MUST re-read mandatory preflight docs before each user prompt.
2. Agents MUST cite file paths and line numbers for repo-specific claims.
3. Agents MUST verify claims with commands/tests before concluding success.
4. Agents MUST NOT invent fallback behavior when code/config is invalid.

## 2. Change Classification Contract

Every task MUST be classified before edits:

1. `foundation-hardening`: governance, safety, contracts, quality gates.
2. `feature-delivery`: behavior visible to users or integrations.
3. `maintenance`: refactor, cleanup, dead-code deletion, docs relocation.

The classification MUST be stated in the working notes or PR description.

## 3. Evidence Contract

Every non-trivial change MUST include:

1. Commands executed
2. Pass/fail outcomes
3. Known gaps or skipped checks
4. Exact artifact paths for generated evidence

For Rust code changes, run the mandatory quality gate from `AGENTS.md`.

Evidence structure requirements are defined by `specs/foundation/verification-evidence-contract.md`.

## 4. Multi-Agent Contract

1. Shared hotspot files MUST have single-lane ownership.
2. Cross-lane changes MUST go through an explicit integration request log.
3. Tier promotion MUST require certification evidence, not assertion.
4. Lane prompts/specs MUST name allowlisted edit paths and validation commands.

## 5. External Contract Discipline

For each external dependency boundary (Horizons, channel APIs, tools):

1. Maintain one canonical contract spec in `specs/`.
2. Include drift triggers and validation strategy.
3. Reject undocumented assumptions in implementation PRs.

## 6. Deletion and Noise Policy

Agents SHOULD delete documentation that is stale, duplicative, or misplaced, provided:

1. A canonical replacement exists, or
2. The file has no active references and no current operational value.

Do not retain “just in case” documentation.

## External References

- NIST SSDF (SP 800-218): https://csrc.nist.gov/pubs/sp/800/218/final
- OpenAPI Specification: https://spec.openapis.org/oas/v3.1.1.html
- JSON Schema Specification: https://json-schema.org/specification
