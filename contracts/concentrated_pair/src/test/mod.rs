// ============================================================================
// Test Modules
// ============================================================================
//
// test/
//   mod.rs        — this file (shared helpers)
//   tick_math.rs  — tick ↔ √price conversion: known values, monotonicity,
//                   round trips, range errors, base-constant derivation
//   initialize.rs — deploy + initialize happy path and every guard
//   ticks.rs      — per-tick storage lifecycle (set/get/delete, TTL policy)
//
// ---------------------------------------------------------------------------

mod initialize;
mod tick_math;
mod ticks;

use soroban_sdk::{testutils::Address as _, Address, Env};

use crate::{ConcentratedPair, ConcentratedPairClient};

/// Q64.64 representation of √1 — price 1.0, tick 0.
pub const SQRT_PRICE_ONE: u128 = 1u128 << 64;

pub fn setup_env() -> (Env, Address, Address, Address, Address) {
    let env = Env::default();
    let contract_id = env.register(ConcentratedPair, ());
    let factory = Address::generate(&env);
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);
    (env, contract_id, factory, token_a, token_b)
}

/// Deploys and initializes a pool at price 1.0 with a 30 bps / spacing 60
/// configuration, returning the client.
pub fn setup_initialized<'a>(env: &'a Env, contract_id: &'a Address) -> ConcentratedPairClient<'a> {
    let factory = Address::generate(env);
    let token_a = Address::generate(env);
    let token_b = Address::generate(env);
    let client = ConcentratedPairClient::new(env, contract_id);
    client.initialize(&factory, &token_a, &token_b, &30, &60, &SQRT_PRICE_ONE);
    client
}
