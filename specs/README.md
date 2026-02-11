# Spec System (Canonical)

This directory is the normative source of truth for OpenCraw behavior and architecture.

## Authority Order

When documents conflict, resolve in this order:

1. `AGENTS.md`
2. `standards/*`
3. `specs/foundation/*`
4. Domain specs (`specs/parity/*`, `specs/system/*`, `specs/platform/*`, `specs/adr/*`)
5. `docs/issues/*` (historical evidence, not normative)
6. `guides/*` (operator/user guidance, not normative)

## Normative Language

All requirement terms follow RFC 2119 / RFC 8174 semantics (`MUST`, `MUST NOT`, `SHOULD`, `SHOULD NOT`, `MAY`).

## Required Spec Header

Every new spec MUST include this header block near the top:

- `Status:` draft | proposed | accepted | superseded | archived
- `Owner:` team or handle
- `Last-Reviewed:` YYYY-MM-DD
- `Scope:` what systems are in/out
- `Depends-On:` referenced specs or standards

## Naming Rules

- Use specific names. Avoid vague names such as `notes`, `misc`, `thoughts`, `todo`.
- Keep filenames stable once referenced by other specs or scripts.
- Prefer role-specific names, e.g. `horizons-api-master-contract.md` over `horizons-api.md`.

## Spec Acceptance Minimum

A spec is not `accepted` unless it defines:

1. Problem statement
2. Explicit constraints
3. Success criteria
4. Verification method (commands/tests/evidence)
5. Rollback or deprecation path

## Cross-Repo Contracts

Any spec that depends on external systems (e.g., Horizons, provider APIs) MUST:

1. Pin the source snapshot or version.
2. Define a regeneration trigger.
3. Define verification commands that detect drift.

## External References

- RFC 2119: https://www.rfc-editor.org/rfc/rfc2119
- RFC 8174: https://www.rfc-editor.org/rfc/rfc8174
