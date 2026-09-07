# Contributing to CoralSwap Core

Thank you for your interest in contributing. This document covers the setup, standards, and process for contributing to the CoralSwap smart contracts.

## Prerequisites

- **Rust** (stable) -- install via [rustup](https://rustup.rs/)
- **wasm32v1-none target** -- `rustup target add wasm32v1-none`
- **Soroban CLI** -- `cargo install soroban-cli`
- **Git** with commit signing recommended

## Local Setup

```bash
git clone https://github.com/CoralSwap-Finance/coralswap-core.git
cd coralswap-core
cargo build
cargo test
```

## Project Structure

```
contracts/
  factory/     -- Pair deployment and protocol governance
  pair/        -- Core AMM logic, dynamic fees, flash loans, TWAP oracle
  lp_token/    -- SEP-41 compliant LP token
  router/      -- User-facing swap and liquidity entry points
  flash_receiver_interface/  -- Flash loan callback trait
tests/         -- Integration tests
```

## Coding Standards

- Run `cargo fmt --all` before committing
- Run `cargo clippy --all-targets -- -D warnings` -- zero warnings allowed
- All public functions must have `/// doc` comments
- Use `i128` for all token amounts (Soroban standard)
- Prefer `Result<T, ContractError>` over panics
- Keep WASM binary size under 64KB per contract

## Commit Messages

Use conventional commits in past active voice:

```
feat(pair): implemented constant-product swap logic
fix(factory): resolved duplicate pair creation check
test(pair): added edge-case tests for mint overflow
docs(pair): added rustdoc for public swap functions
refactor(router): extracted deadline validation helper
```

**Format:** `type(scope): description`

**Types:** `feat`, `fix`, `test`, `docs`, `refactor`, `chore`, `ci`

**Scopes:** `pair`, `factory`, `router`, `lp-token`, `flash`

## Pull Request Process

1. Fork the repo and create a branch: `feat/issue-NUMBER-short-description`
2. Make your changes following the standards above
3. Ensure CI passes: `cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings && cargo test`
4. Open a PR against `main` using the PR template
5. Reference the issue number in your PR description
6. Wait for review -- first response within 24 hours

## Testing

- Unit tests go in `contracts/<name>/src/test/`
- Integration tests go in `tests/`
- Use `soroban_sdk::testutils` for test environments
- All new functions must have corresponding tests

## Continuous Integration

- The `CI` workflow runs formatting, clippy, build, and tests on every push and PR.
- The `SDK Matrix` workflow (`.github/workflows/matrix.yml`) builds and tests the
  contracts against multiple `soroban-sdk` versions to catch breaking changes early:
  - The pinned stable version (matching `Cargo.toml`) is **required** to pass.
  - Newer stable and preview/RC versions are **warn-only** -- their failures are
    allowed but reported as annotations and in the run summary.
- It also runs on a weekly schedule and via manual `workflow_dispatch`. When adopting
  a newer SDK, bump the pinned versions in `Cargo.toml` and the matrix entries together.

## Security

- Never commit secrets, keys, or `.env` files
- Report vulnerabilities privately via GitHub Security Advisories
- All token math must use checked arithmetic or validated `i128` ranges

### Soroban Security Model

> **Important:** If you are contributing to any state-mutating function in the
> `pair` contract — especially functions that make cross-contract calls — read
> [ARCHITECTURE.md § Soroban Reentrancy Model](ARCHITECTURE.md#soroban-reentrancy-model)
> before writing or reviewing code.

Soroban's single-frame, copy-on-write execution model is **not** equivalent to
Solidity's shared mutable world state. Key differences that affect how you
write safe contract code:

- **No ETH-style implicit callbacks.** Token transfers are explicit
  cross-contract calls; there is no fallback function to exploit.
- **Host abort rolls back atomically.** A panicking sub-call discards all
  writes in the current invocation frame — there is no partial commit to
  ledger state.
- **Storage-based reentrancy guard is still required.** Within a single
  invocation, sub-calls to the same contract observe its current working-set
  writes. A reentrant call during the flash-loan callback window can observe
  and exploit intermediate, not-yet-validated state.
- **RAII lock pattern.** The `ReentrancyGuard` in
  `contracts/pair/src/reentrancy.rs` uses Rust's `Drop` trait to guarantee
  unconditional lock release — do not call `release_lock` manually and do not
  bypass the guard with raw storage writes to `DataKey::Guard`.

## License

By contributing, you agree that your contributions will be licensed under the project's MIT License.
