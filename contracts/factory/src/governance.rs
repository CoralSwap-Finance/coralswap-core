use crate::errors::FactoryError;
use soroban_sdk::{Address, Env, Vec};

/// Minimum number of distinct registered signers needed to act on a governance
/// function when `signer_count` signers are registered.
///
/// # Quorum scale (issue #364)
///
/// Quorum is a **strict majority**: `signatures > n / 2`, i.e.
/// `threshold = n / 2 + 1` (integer division). Exactly half is *not* a quorum,
/// so two disjoint halves of an even-sized set can never both act.
///
/// | signers `n` | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 10 |
/// |-------------|---|---|---|---|---|---|---|---|---|----|
/// | quorum      | 1 | 2 | 2 | 3 | 3 | 4 | 4 | 5 | 5 |  6 |
///
/// Applies uniformly to every multisig-gated function: `pause`, `unpause`,
/// `freeze_pair`, `unfreeze_pair`, `propose_upgrade`, `cancel_upgrade`.
///
/// The set of signers is fixed at `initialize`, so the threshold cannot be
/// lowered by shrinking the set at runtime; the function is pure so that the
/// `n -> n - 1` behaviour a future signer-rotation feature would have is pinned
/// down by tests today.
pub const fn quorum_threshold(signer_count: u32) -> u32 {
    signer_count / 2 + 1
}

/// Verifies that a quorum of the **registered** signers has authorized the
/// current invocation.
///
/// * `registered` — the factory's stored signer set (source of truth).
/// * `signers` — the addresses presented by the caller for this call.
///
/// Each presented address must be a registered signer and may appear only
/// once; only then does it count toward the quorum given by
/// [`quorum_threshold`]. Addresses outside the registered set do **not**
/// count, and can not be used to pad the list up to the threshold. Every
/// counted signer then has `require_auth()` called, which panics (rolling back
/// the tx) if that signer's authorization is missing.
///
/// # Errors
/// * `InvalidSignerCount` — `registered` is empty (nothing could reach quorum).
/// * `Unauthorized` — a presented address is not a registered signer.
/// * `DuplicateSigner` — the same address is presented more than once.
/// * `InsufficientSignatures` — fewer distinct registered signers than quorum.
pub fn verify_multisig(
    _env: &Env,
    registered: &Vec<Address>,
    signers: &Vec<Address>,
) -> Result<(), FactoryError> {
    let required = quorum_threshold(registered.len());
    if registered.is_empty() {
        return Err(FactoryError::InvalidSignerCount);
    }

    // Validate the presented list before requiring any auth.
    for (i, signer) in signers.iter().enumerate() {
        if !registered.contains(&signer) {
            return Err(FactoryError::Unauthorized);
        }
        // Earlier entries only: O(n^2) over at most 10 signers.
        for j in 0..(i as u32) {
            if signers.get_unchecked(j) == signer {
                return Err(FactoryError::DuplicateSigner);
            }
        }
    }

    // Every presented signer is registered and distinct, so the length *is*
    // the count of distinct registered signers.
    if signers.len() < required {
        return Err(FactoryError::InsufficientSignatures);
    }

    for signer in signers.iter() {
        signer.require_auth();
    }

    Ok(())
}
