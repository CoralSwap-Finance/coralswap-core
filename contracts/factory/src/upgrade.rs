use crate::errors::FactoryError;
use crate::storage::{self, PendingUpgrade};
use soroban_sdk::{BytesN, Env};

/// 72 hours expressed in ledgers, assuming a ~5-second ledger close time.
/// 72 * 3600 / 5 = 51_840 ledgers.
const UPGRADE_DELAY_LEDGERS: u32 = 51_840;

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
pub fn execute_upgrade(env: &Env) -> Result<(), FactoryError> {
    let proposal =
        storage::get_pending_upgrade(env).ok_or(FactoryError::NoPendingUpgrade)?;

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
