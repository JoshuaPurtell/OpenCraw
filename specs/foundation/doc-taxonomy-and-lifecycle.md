# Documentation Taxonomy and Lifecycle

- Status: accepted
- Owner: platform
- Last-Reviewed: 2026-02-11
- Scope: markdown documentation placement and retention
- Depends-On: `specs/README.md`, `specs/foundation/agent-guardrails-contract.md`

## 1. Allowed Documentation Classes

Only these classes are allowed:

1. `standards/*`:
   Mandatory engineering policy and quality bars.
2. `specs/*`:
   Normative system contracts and architecture behavior.
3. `guides/*`:
   User/operator how-to documentation.
4. `docs/plans/*`:
   Active, time-bound execution plans only.
5. `docs/issues/*`:
   Incident reports, audits, retrospectives, historical evidence.

No other markdown buckets are allowed as long-term storage.

## 2. Plan Qualification Rules

A document MAY live in `docs/plans/` only if all are true:

1. It has an active owner.
2. It has explicit milestone or completion criteria.
3. It has a target time window.
4. It is not superseded.

If any condition fails, move it to `docs/issues/` (history) or delete it.

## 3. Spec Qualification Rules

A document MUST live in `specs/` when it defines:

1. Behavioral contract
2. API/schema contract
3. Architecture boundary or responsibility split
4. Canonical source-of-truth mapping

## 4. Guide Qualification Rules

A document MUST live in `guides/` when it teaches a user or operator workflow.

Guides are non-normative and must not redefine policy already in `standards/` or `specs/`.

## 5. Issue Qualification Rules

A document MUST live in `docs/issues/` when it records:

1. RCA / incident
2. Audit findings
3. Historical backlog snapshots
4. Research notes that are no longer normative

## 6. Retention and Deletion

Docs SHOULD be deleted when they are:

1. Frontend/web-only artifacts outside current product scope
2. Duplicates of canonical specs
3. Generated dumps without ongoing operational use
4. Unreferenced and older than one review cycle

## 7. Review Cadence

1. `specs/*` MUST be reviewed when upstream contracts change.
2. `docs/plans/*` MUST be reviewed at least weekly while active.
3. `docs/issues/*` are immutable records except metadata corrections.

## External References

- Diataxis documentation framework: https://diataxis.fr/
