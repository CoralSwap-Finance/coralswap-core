// ============================================================================
// Test Modules
// ============================================================================
//
// Each submodule contains a focused set of unit tests for one aspect of the
// Pair contract.  All modules live under `contracts/pair/src/test/` and are
// gated behind `#[cfg(test)]` via the parent `lib.rs`.
//
// test/
//   mod.rs           — this file (re-exports + shared helpers)
//   swap_math.rs     — pure-function tests for swap math, fee deduction,
//                      K-invariant, overflow, symmetry, etc.
//   events.rs        — PairEvents emission assertions for every event type
//   dynamic_fee.rs   — unit tests for dynamic fee engine (volatility, decay)
//   sync.rs          — tests for reserve synchronization (Pair::sync)
//   reentrancy.rs    — tests for reentrancy guard (acquire/release)
//
// ---------------------------------------------------------------------------

mod events;
mod initialize;
mod mint;
mod swap_math;
mod views;
