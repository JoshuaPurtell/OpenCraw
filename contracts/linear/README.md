# Linear API Contract Snapshot

Generated (UTC): 2026-02-11T20:53:39Z  
GraphQL endpoint: `https://api.linear.app/graphql`  
Schema SHA256: `f8f75b92ede8c4267cf4e48a9a30309700ab816c6a21a3aebe5452b52f121ec1`

This directory stores a reproducible snapshot of the **full Linear GraphQL introspection contract** and extracted contracts used by OpenCraw.

## Files

- `introspection-full.json`: full `__schema` introspection payload.
- `schema-metadata.json`: snapshot metadata and contract counts.
- `query-fields.json`: root query field contracts (args + return types).
- `mutation-fields.json`: root mutation field contracts (args + return types).
- `subscription-fields.json`: root subscription contracts (if exposed).
- `input-objects.json`: every GraphQL input object contract.
- `enum-types.json`: every enum type + allowed values.
- `state-related-enums.json`: enums with `state`/`status` in their type name.
- `scalars.json`: scalar type inventory.
- `directives.json`: directive contracts.

## Regenerate

```bash
scripts/fetch-linear-contracts.sh
```

## State/Status enums detected

- AgentSessionStatus: pending, active, complete, awaitingInput, error, stale
- CustomerStatusType: active, inactive
- GitAutomationStates: draft, start, review, mergeable, merge
- InitiativeStatus: Planned, Active, Completed
- IssueSuggestionState: active, stale, accepted, dismissed
- OAuthClientApprovalStatus: requested, approved, denied
- OrganizationInviteStatus: pending, accepted, expired
- ProjectMilestoneStatus: unstarted, next, overdue, done
- ProjectStatusType: backlog, planned, started, paused, completed, canceled
- PullRequestStatus: draft, open, inReview, approved, merged, closed
- SlaStatus: Breached, HighRisk, MediumRisk, LowRisk, Completed, Failed
- SummaryGenerationStatus: pending, completed, failed

## Notes

- This is the source of truth for schema-level contracts.
- Some runtime validation constraints may exist beyond introspection and are surfaced as GraphQL errors at execution time.
