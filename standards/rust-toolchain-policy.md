# Rust Toolchain and Build Policy

Date: 2026-02-11
Status: active
Owner: platform

## Goal

Define deterministic Rust toolchain, lint, and release-build policy independent of CI implementation details.

## 1) Toolchain Pinning

1. Repository MUST include `rust-toolchain.toml` at root.
2. Toolchain channel MUST be pinned to an explicit stable version (for example `1.85.0`), not a floating alias.
3. `rustfmt` and `clippy` components MUST be pinned with the toolchain.
4. Toolchain changes MUST be made in foundation-hardening changes with full gate evidence.

## 2) Workspace Version Contract

1. Root `Cargo.toml` MUST declare workspace-level `edition`, `resolver`, and `rust-version`.
2. `rust-version` MUST be less than or equal to the pinned toolchain.
3. Raising `rust-version` requires coordinated toolchain update and full verification evidence.

## 3) Lint Policy Contract

1. Workspace lint policy MUST be explicit (`[workspace.lints.rust]` and `[workspace.lints.clippy]` or equivalent crate-level policy).
2. Unsafe code policy MUST be explicit and default-deny (`forbid` preferred).
3. Any lint waiver MUST include narrow scope and rationale.

## 4) Release Profile Contract

1. Root `Cargo.toml` MUST explicitly define `[profile.release]`.
2. Release profile MUST explicitly set overflow behavior.
3. Release profile SHOULD explicitly set panic strategy, LTO strategy, and codegen-units.
4. Any deviation from the selected baseline requires documented rationale in review artifacts.

## 5) Reproducibility Contract

1. Required gates use lockfile-pinned execution by default (`--locked`; `--frozen` where no lockfile mutation is acceptable).
2. Latest-dependency verification runs in a separate scheduled lane without lockfile pinning.
3. Dependencies pulled from git SHOULD be revision-pinned; branch tracking requires explicit drift contract in specs.

## 6) Upgrade and Drift Procedure

1. Open a foundation-hardening change for toolchain/profile/lint updates.
2. Run required quality gates and record evidence.
3. Record impact in standards/spec artifacts before merge.
4. Remove temporary waivers as part of the same or immediate follow-up change.
