# Standards (Authoritative)

This directory defines mandatory engineering policy for OpenCraw.

## Authority

If implementation details conflict with standards, standards win.

## Current Standards

1. `standards/rust.md`: Rust engineering baseline and architecture guardrails.
2. `standards/rust-toolchain-policy.md`: deterministic toolchain/lint/release policy.
3. `standards/foundation-gates.md`: required quality gates and waiver requirements.
4. `standards/verification-governance.md`: runner-agnostic verification and evidence governance.

## Enforcement Model

Standards are enforced by evidence. CI is one execution lane, not the policy source.
