//! Cross-contract client used by the pair to consult the factory for the
//! per-pair fee override (issue #132).
//!
//! The trait mirrors the relevant subset of the factory's public surface; the
//! pair only ever needs to read the override, never to mutate it. Defining the
//! trait locally keeps the pair crate free of a circular dependency on the
//! factory.

#![allow(dead_code)]

use soroban_sdk::{contractclient, Address, Env};

/// Subset of `Factory`'s public methods consulted by the pair during swap.
///
/// `contractclient` generates a typed `FactoryClient` that performs a
/// `try_*` cross-contract invocation under the hood. The pair always uses
/// `try_*` so a missing override (or a transient factory error) degrades
/// gracefully to the dynamic-fee path instead of reverting the swap.
#[contractclient(name = "FactoryClient")]
pub trait FactoryInterface {
    /// Returns the per-pair fee override in basis points, or `None` if no
    /// override is set. See `Factory::set_pair_fee`.
    fn get_pair_fee_override(env: Env, pair: Address) -> Option<u32>;
}
