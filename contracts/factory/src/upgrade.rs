//! Timelocked WASM upgrade path (issue #344).
//!
//! # Timelock enforcement
//!
//! A governance upgrade is a two-step process, and the delay between the two
//! steps is the whole point of the mechanism:
//!
//! 1. [`propose_upgrade`] — multisig-gated. Stores the new WASM hash together
//!    with the ledger sequence the proposal was made at
//!    ([`crate::storage::PendingUpgrade`]) and emits `prop_upg`.
//! 2. [`execute_upgrade`] — permissionless *but only after*
//!    [`UPGRADE_DELAY_LEDGERS`] have elapsed since `proposed_at_ledger`. It
//!    computes `elapsed = current_ledger - proposed_at_ledger` and reverts
//!    with [`FactoryError::UpgradeTimelockNotExpired`] while
//!    `elapsed < UPGRADE_DELAY_LEDGERS`. The boundary is inclusive: exactly
//!    `UPGRADE_DELAY_LEDGERS` ledgers of elapsed time is enough to execute.
//! 3. [`cancel_upgrade`] — multisig-gated escape hatch that clears a pending
//!    proposal at any point during the delay.
//!
//! The delay is 72 hours so token holders and integrators have a realistic
//! window to inspect a proposed upgrade (via the `pending_upgrade` view),
//! react (governance can `cancel_upgrade`), or exit before the new code takes
//! effect. Because `execute_upgrade` is permissionless once the delay has
//! elapsed, no single key is needed to complete a queued upgrade.
//!
//! All three paths are covered by the timelock tests in
//! `crate::test::factory_tests` (issue #344).

use crate::errors::FactoryError;
use crate::storage::{self, PendingUpgrade};
use soroban_sdk::{BytesN, Env};

/// 72 hours expressed in ledgers, assuming a ~5-second ledger close time.
/// 72 * 3600 / 5 = 51_840 ledgers.
///
/// This is the minimum number of ledgers that must pass between
/// `propose_upgrade` and `execute_upgrade`; see the module documentation for
/// the full enforcement semantics. Exposed on-chain through the
/// `Factory::upgrade_delay_ledgers` view so off-chain tooling never has to
/// hard-code the assumption (issue #344).
pub const UPGRADE_DELAY_LEDGERS: u32 = 51_840;

/// Proposes a WASM upgrade. Stores the hash and the current ledger sequence.
/// Rejects if a proposal is already pending.
///
/// Must be called after `verify_multisig()` has been satisfied by the caller.
pub fn propose_upgrade(env: &Env, new_wasm_hash: BytesN<32>) -> Result<(), FactoryError> {
    if storage::get_pending_upgrade(env).is_some() {
        return Err(FactoryError::UpgradeAlreadyPending);
    }

    let proposal = PendingUpgrade {
        new_wasm_hash: new_wasm_hash.clone(),
        proposed_at_ledger: env.ledger().sequence(),
    };
    storage::set_pending_upgrade(env, &proposal);

    crate::events::FactoryEvents::upgrade_proposed(env, &new_wasm_hash.to_array());
    Ok(())
}

/// Executes a previously proposed upgrade after the 72-hour timelock has
/// elapsed. Reverts with `UpgradeTimelockNotExpired` if called too early.
///
/// # Timelock enforcement (issue #344)
///
/// `elapsed = current_ledger - proposed_at_ledger` must be at least
/// [`UPGRADE_DELAY_LEDGERS`]; one ledger short reverts and leaves the pending
/// proposal (and every other governance path) untouched. Anyone may call this
/// once the delay has elapsed — the delay, not a key, is the gate.
pub fn execute_upgrade(env: &Env) -> Result<(), FactoryError> {
    let proposal = storage::get_pending_upgrade(env).ok_or(FactoryError::NoPendingUpgrade)?;

    let elapsed = env.ledger().sequence().saturating_sub(proposal.proposed_at_ledger);
    if elapsed < UPGRADE_DELAY_LEDGERS {
        return Err(FactoryError::UpgradeTimelockNotExpired);
    }

    // Apply the upgrade.
    env.deployer().update_current_contract_wasm(proposal.new_wasm_hash);

    // Clear the pending proposal.
    storage::remove_pending_upgrade(env);

    // Bump the protocol version in factory storage.
    if let Some(mut fs) = storage::get_factory_storage(env) {
        fs.protocol_version += 1;
        storage::set_factory_storage(env, &fs);
        crate::events::FactoryEvents::upgrade_executed(env, fs.protocol_version);
    }

    Ok(())
}

/// Cancels a pending upgrade proposal. Must be called after multisig check.
pub fn cancel_upgrade(env: &Env) -> Result<(), FactoryError> {
    if storage::get_pending_upgrade(env).is_none() {
        return Err(FactoryError::NoPendingUpgrade);
    }
    storage::remove_pending_upgrade(env);
    Ok(())
}
