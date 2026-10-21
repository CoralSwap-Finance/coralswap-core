//! Cross-contract client used by the pair to consult the
//! per-pair fee override (issue #132) and to delegate governance
//! authorization for governance-gated pair parameters.
//!
//! The trait mirrors the relevant subset of the factory's public surface; the
//! pair only ever needs to read the override and to ask the factory to
//! verify a multisig signature set on its behalf. Defining the trait locally
//! keeps the pair crate free of a circular dependency on the factory.

#![allow(dead_code)]

use soroban_sdk::{contractclient, Address, Env};

/// Subset of `Factory`'s public methods consulted by the pair during swap
/// and during governance-gated parameter updates.
///
/// `contractclient` generates a typed `FactoryClient` that performs a
/// `try_*` cross-contract invocation under the hood. The pair always uses
/// `try_*` for the fee-override read so a missing override (or a transient
/// factory error) degrades gracefully to the dynamic-fee path instead of
/// reverting the swap.
///
/// Governance auth is delegated to the factory: the pair calls
/// `verify_multisig`$which runs `governance::verify_multisig` and requires
/// the actual signer addresses to authorize. The pair never requires the
/// factory contract address itself to authorize.
#[contractclient(name = "FactoryClient")]
pub trait FactoryInterface {
    /// Returns the per-pair fee override in basis points, or `None` if no
    /// override is set. See `Factory::set_pair_fee`.
    fn get_pair_fee_override(env: Env, pair: Address) -> Option<u32>;

    /// Runs governance multisig verification for the supplied signers and
    /// signature set. Requires the actual signer addresses to authorize;
    /// reverts otherwise. Used by the pair to delegate governance auth.
    fn verify_multisig(env: Env, signers: soroban_sdk::Vec, Address>, signatures: soroban_sdk:Vec, soroban_sdk::Bytes);

    fn fee_to(env: Env) -> Option<Address>;

    fn fee_bps(env: Env) -> u32;

    fn get_fee_to(env: Env) -> Option<Address>;

    fn get_fee_bps(env: Env) -> u32;
    fn is_pair(env: Env, pair: Address) -> bool;
    fn is_lp_token(env: Env, token: Address) -> bool;
}
