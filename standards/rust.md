# Rust Foundation Best Practices (Internet-Reviewed)

Date: 2026-02-11
Status: Baseline quality contract for Zangbot

## Goal

Define the minimum engineering foundation that must be in place before feature velocity.

This document is strict on purpose: the objective is to reduce future refactors, not maximize short-term speed.

## 1) Workspace and Toolchain Governance

1. Use `edition = "2024"` and `resolver = "3"` at workspace root.
2. Set and maintain an explicit `rust-version` policy in `Cargo.toml`.
3. Verify latest dependency resolution in a scheduled verification lane (separate from lockfile builds).

Why:

- Rust 2024 implies resolver v3 (MSRV-aware behavior), and resolver is a workspace-global setting.
- Cargo recommends verifying against latest dependencies in continuous verification.

## 2) Lint and Compiler Discipline

1. Enforce clippy in required verification gates with at least `clippy::all` and `clippy::correctness`.
2. Treat correctness/soundness lints as build blockers.
3. Use explicit lint level strategy (`warn`/`deny`/`forbid`) deliberately.

Why:

- `rustc` has explicit lint-level semantics (`allow`, `expect`, `warn`, `force-warn`, `deny`, `forbid`).
- `clippy::correctness` targets code that is outright wrong or useless.

## 3) Domain Modeling Standards (Data First)

1. Use newtypes and domain value objects for all semantically distinct primitives.
2. Express invalid-state prevention in types when practical.
3. Validate all boundary input aggressively; do not "accept and interpret later".

Why:

- Rust API Guidelines recommend static distinctions with newtypes and validating arguments.
- This directly supports a data-first architecture and prevents category errors in finance flows.

## 4) Error Handling Policy

1. Use `Result` for recoverable errors; reserve `panic!` for unrecoverable bugs.
2. Add contextual error information at each boundary hop.
3. Avoid `unwrap`/`expect` in production paths (tests and tightly proven invariants excepted).

Why:

- The Rust Book distinguishes recoverable (`Result`) vs unrecoverable (`panic!`) errors.
- Contextual error chains significantly improve production diagnosis.

## 5) Concurrency and Async Safety

1. Design with `Send`/`Sync` semantics explicit in shared types.
2. Implement graceful shutdown as a first-class workflow: signal, broadcast, drain, wait.
3. Use cancellation tokens for coordinated task cancellation.
4. Use `spawn_blocking` only for truly blocking/CPU work and bound parallelism externally.

Why:

- Tokio’s shutdown model is explicit and battle-tested.
- `spawn_blocking` has caveats: blocking jobs are not abruptly abortable and require concurrency control.

## 6) HTTP Runtime Guardrails

1. Apply per-route timeout policy.
2. Apply global concurrency caps to protect dependencies and tail latency.
3. Keep error types stable at HTTP boundaries (prefer explicit responses to transport-level termination).

Why:

- `tower_http::timeout` is designed for HTTP services and avoids error-type widening behavior from `tower::timeout`.
- Global concurrency limits can be shared across services using one semaphore.

## 7) Numeric and Runtime Safety Defaults

1. Never use floating point for monetary values.
2. Explicitly decide overflow behavior in release profile; document it.
3. Keep runtime assertions where they protect invariants that cannot be encoded statically.

Why:

- Cargo release defaults include `overflow-checks = false`; this must be a conscious decision, not an accident.

## 8) Database Correctness Guardrails

1. Prefer compile-time checked SQL for core queries.
2. Enforce query metadata freshness in verification gates (`cargo sqlx prepare --check --workspace`).
3. Use offline metadata mode in verification gates to keep builds deterministic.

Why:

- SQLx query macros validate SQL against the actual schema at build time.
- `prepare --check` fails verification when schema/query metadata is stale.

## 9) Unsafe Code and Undefined Behavior Strategy

1. Default to `#![forbid(unsafe_code)]`.
2. If unsafe is required, isolate it in tiny modules with explicit invariants and tests.
3. Add a UB-detection test lane with Miri for critical crates.

Why:

- Miri is a UB detection tool and catches classes of unsafe misuse during test execution.

## 10) Supply Chain and Security Gates

1. Run `cargo-audit` regularly in verification lanes.
2. Run policy checks with `cargo-deny` (advisories, licenses, source policies, duplicate crates).
3. Track remediation SLAs for security advisories.
4. Optional high-assurance layer: use `cargo-vet` for trust/audit attestations on third-party crates.

Why:

- RustSec tooling is designed exactly for this operational model.

## 11) Testing Pyramid for OpenCraw

1. Unit tests for domain invariants and state transitions.
2. Integration tests for workflows and boundary adapters.
3. Property tests for monetary and lifecycle invariants.
4. Concurrency tests for worker idempotency/ordering races.
5. UB/soundness lane (Miri) for crates with unsafe surface or tricky concurrency assumptions.

Why:

- Rust Book explicitly frames tests as required beyond type checking for correctness.

## 12) Verification Contract (Required before scale)

Minimum required checks:

1. `cargo fmt --check`
2. `cargo clippy --workspace --all-targets --all-features -- -D warnings`
3. `cargo test --workspace --all-features`
4. `cargo sqlx prepare --check --workspace` (if SQLx query macros are used)
5. `cargo audit`
6. Optional scheduled lane: latest-deps verification and Miri subset

Latest-deps lane should follow Cargo guidance for continuous verification (separate from lockfile-pinned builds).

## 13) Non-Negotiable Architecture Guardrails

1. Domain crates cannot import HTTP, DB drivers, queue clients, or SDK clients.
2. Interface adapters cannot contain business policy.
3. Shared code must be either truly cross-domain utility or deleted.
4. Every cross-boundary command must be idempotent.

This is how we keep the system clean at scale while preserving DRY without creating abstraction sludge.

## 14) Adoption Plan (Immediate)

1. Create a `FOUNDATION_GATES.md` and enforce it through approved verification runners.
2. Add lint/profile/toolchain policy in first commit of implementation.
3. Block feature merges until core gates are green.
4. Revisit only when evidence shows a gate is net-negative.

## Sources

- Rust Edition Guide: Cargo resolver v3 (`edition = "2024"`) and CI recommendation
  - https://doc.rust-lang.org/stable/edition-guide/rust-2024/cargo-resolver.html
- Cargo Book: profiles, overflow checks, release defaults, workspace profile behavior
  - https://doc.rust-lang.org/cargo/reference/profiles.html
- Cargo Book: `rust-version` policy and support expectations
  - https://doc.rust-lang.org/cargo/reference/rust-version.html
- Rustc Book: lint levels (`allow` -> `forbid`)
  - https://doc.rust-lang.org/rustc/lints/levels.html
- Clippy docs: lint categories and `clippy::correctness`
  - https://doc.rust-lang.org/clippy/index.html
- Rust API Guidelines: type safety and dependability (`C-NEWTYPE`, `C-VALIDATE`, destructor guidance)
  - https://rust-lang.github.io/api-guidelines/type-safety.html
  - https://rust-lang.github.io/api-guidelines/dependability.html
- Rust Book: error handling, testing, concurrency traits (`Send`/`Sync`)
  - https://doc.rust-lang.org/stable/book/ch09-00-error-handling.html
  - https://doc.rust-lang.org/stable/book/ch11-00-testing.html
  - https://doc.rust-lang.org/book/ch16-04-extensible-concurrency-sync-and-send.html
- Tokio docs: graceful shutdown and tracing
  - https://tokio.rs/tokio/topics/shutdown
  - https://tokio.rs/tokio/topics/tracing
- Tokio and tower docs: cancellation, blocking boundaries, timeout/concurrency limits
  - https://docs.rs/tokio-util/latest/tokio_util/sync/struct.CancellationToken.html
  - https://docs.rs/tokio/latest/tokio/task/fn.spawn_blocking.html
  - https://docs.rs/tower-http/latest/tower_http/timeout/index.html
  - https://docs.rs/tower/latest/tower/limit/concurrency/struct.GlobalConcurrencyLimitLayer.html
- SQLx docs and CLI docs
  - https://github.com/launchbadge/sqlx
  - https://docs.rs/crate/sqlx-cli/latest/source/README.md
- Miri
  - https://github.com/rust-lang/miri
- RustSec tooling
  - https://rustsec.org/
- Cargo CI guidance (latest dependency verification and rust-version checks)
  - https://doc.rust-lang.org/cargo/guide/continuous-integration.html
- cargo-vet (optional high-assurance supply-chain control)
  - https://github.com/mozilla/cargo-vet
