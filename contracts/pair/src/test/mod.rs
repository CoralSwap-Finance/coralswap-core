// ============================================================================
// Test Modules
// ============================================================================
//
// Each submodule contains a focused set of unit tests for one aspect of the
// Pair contract.  All modules live under `contracts/pair/src/test/` and are
// gated behind `#[cfg(test)]` via the parent `lib.rs`.
//
// test/
//   mod.rs              — this file (re-exports + shared helpers)
//   swap_math.rs        — pure-function tests for swap math, fee deduction,
//                         K-invariant, overflow, symmetry, etc.
//   events.rs           — PairEvents emission assertions for every event type
//   dynamic_fee.rs      — unit tests for dynamic fee engine (volatility, decay)
//   sync.rs             — tests for reserve synchronization (Pair::sync)
//   reentrancy.rs       — tests for reentrancy guard (acquire/release)
//   pair_fee_override.rs — tests for per-pair fee override wiring (issue #132)
//
// ---------------------------------------------------------------------------

mod burn;
mod dynamic_fee;
mod events;
mod flash_loan;
mod initialize;
mod mint;
mod mint_single_side;
mod oracle;
mod pair_fee_override;
mod reentrancy;
mod swap_math;
mod sync;
mod views;
