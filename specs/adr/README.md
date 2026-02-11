# Architecture Decision Records (ADR)

Use ADRs for any change with non-trivial blast radius or non-obvious tradeoffs.

## Rules

1. One ADR per decision.
2. Status must be one of: `proposed`, `accepted`, `superseded`, `rejected`.
3. Every risky PR must reference an ADR id or explicitly state why ADR is not needed.
4. If a decision is superseded, add the replacement ADR id and date.

## Naming

1. File format: `NNNN-short-title.md`.
2. Start with `0001` and increment by one.
3. Keep titles concrete and specific.

## Template

Copy `specs/adr/0000-template.md` and fill every section.
