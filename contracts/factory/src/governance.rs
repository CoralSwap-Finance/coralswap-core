use crate::errors::FactoryError;
use soroban_sdk::{Address, Env, Vec};

/// Verifies that at least `required` signers from `signers` have authorized
/// the current invocation. Each signer in the list calls `require_auth()`,
/// which panics (and rolls back the tx) if the authorization is missing.
///
/// Returns `InsufficientSignatures` if fewer than `required` signers are
/// provided, or `InvalidSignerCount` if `required` is zero.
pub fn verify_multisig(
    _env: &Env,
    signers: &Vec<Address>,
    required: u32,
) -> Result<(), FactoryError> {
    if required == 0 {
        return Err(FactoryError::InvalidSignerCount);
    }

    let provided = signers.len();
    if provided < required {
        return Err(FactoryError::InsufficientSignatures);
    }

    // Require authorization from each provided signer.
    for signer in signers.iter() {
        signer.require_auth();
    }

    Ok(())
}
