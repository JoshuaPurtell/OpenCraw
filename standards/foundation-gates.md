# OpenCraw Foundation Gates

Date: 2026-02-09  
Source standards:

- `standards/rust.md` (origin: zangbot `plans/14-rust-foundation-best-practices.md`)
- `standards/foundation-gates.md` (origin: zangbot `plans/15-divine-engineering-standard.md`)

## Required Merge Gates

All pull requests must pass:

1. `cargo fmt --all -- --check`
2. `cargo check --workspace --all-targets --locked`
3. `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
4. `cargo test --workspace --all-targets --locked`
5. `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked`
6. `cargo audit`
7. `cargo deny check advisories bans licenses sources`

## Scheduled Verification Lane

Weekly latest-dependency verification:

1. `cargo update`
2. `cargo check --workspace --all-targets`
3. `cargo clippy --workspace --all-targets --all-features -- -D warnings`
4. `cargo test --workspace --all-targets`

## Runtime Guardrails (Required)

1. HTTP timeout middleware is enabled globally.
2. Global in-flight request limit is enabled.
3. Graceful shutdown is signal-driven (`SIGINT`/`SIGTERM`), with cancellation broadcast and task drain.

## Waiver Rule

Any exception to these gates requires:

1. Owner
2. Scope
3. Expiry date
4. Compensating control
5. Rollback plan
