use soroban_sdk::{token::TokenClient, Address, Bytes, Env};

use coralswap_flash_receiver_interface::FlashReceiverClient;

use crate::{
    errors::PairError,
    events::PairEvents,
    reentrancy,
    storage::{get_fee_state, get_pair_state, set_pair_state},
};

/// Minimum flash-loan fee in basis points (0.05%).
/// The effective fee is max(current_dynamic_fee_bps, FLASH_FEE_FLOOR_BPS).
const FLASH_FEE_FLOOR_BPS: u32 = 5;

/// Maximum allowed byte length for the `data` payload passed to the receiver.
const MAX_PAYLOAD_SIZE: u32 = 256;

/// Computes the flash-loan fee for `amount` stroops.
///
/// The effective fee rate is the higher of the pool's current dynamic fee and
/// the hardcoded floor (`FLASH_FEE_FLOOR_BPS = 5`, i.e. 0.05%).  This ensures
/// flash loans are always revenue-positive for LPs even during low-fee periods.
///
/// A minimum of **1 stroop** is enforced so that zero-fee loans are impossible
/// regardless of rounding.
///
/// # Arguments
/// * `amount`          – Loan principal in stroops (must be > 0).
/// * `current_fee_bps` – Pool's current dynamic fee in basis points.
pub fn compute_flash_fee(amount: i128, current_fee_bps: u32) -> i128 {
    let effective_bps = current_fee_bps.max(FLASH_FEE_FLOOR_BPS) as i128;
    // Use checked_mul to guard against astronomically large loans overflowing
    // i128; saturate to i128::MAX (fee > principal) rather than panicking.
    let fee = amount
        .checked_mul(effective_bps)
        .map(|v| v / 10_000_i128)
        .unwrap_or(i128::MAX);
    // At least 1 stroop to prevent zero-cost loans.
    fee.max(1)
}

/// Executes a dual-token flash loan with full invariant enforcement.
///
/// # Flow
/// 1. **Pre-flight checks** — payload size, amount signs, pair initialized,
///    amounts within reserves.
/// 2. **Reentrancy guard** — acquired before any token movement.
/// 3. **Transfer** — send `amount_a` / `amount_b` to `receiver`.
/// 4. **Callback** — call `receiver.on_flash_loan(...)`.  The receiver MUST
///    repay principal + fee before the callback returns.
/// 5. **Repayment check** — `new_balance >= old_reserve + fee` for each
///    borrowed token.
/// 6. **Reserve update** — set reserves to post-callback balances.
/// 7. **k-invariant** — `post_k >= pre_k`; reverts on violation.
/// 8. **Persist + emit** — write updated state, publish event.
/// 9. **Release lock**.
///
/// # Errors
/// | Error                    | Condition                                          |
/// |--------------------------|---------------------------------------------------|
/// | `FlashPayloadTooLarge`   | `data.len() > MAX_PAYLOAD_SIZE` (256 bytes)       |
/// | `InsufficientInputAmount`| Both amounts are zero, or either is negative      |
/// | `NotInitialized`         | Pair storage not yet written by `initialize`       |
/// | `InsufficientLiquidity`  | Requested amount exceeds current reserves         |
/// | `Locked`                 | Reentrancy — another flash loan is in progress    |
/// | `FlashLoanNotRepaid`     | Post-callback balance < `old_reserve + fee`       |
/// | `InvalidK`               | Post-loan k-invariant is lower than pre-loan      |
/// | `Overflow`               | Arithmetic overflow computing k or required repay |
pub fn execute_flash_loan(
    env: &Env,
    receiver: &Address,
    amount_a: i128,
    amount_b: i128,
    data: &Bytes,
) -> Result<(), PairError> {
    // -----------------------------------------------------------------------
    // 1. Pre-flight checks (no state mutation)
    // -----------------------------------------------------------------------

    // Payload size guard — prevent large data from being used for DOS.
    if data.len() > MAX_PAYLOAD_SIZE {
        return Err(PairError::FlashPayloadTooLarge);
    }

    // At least one token must be borrowed; negative amounts are nonsensical.
    if amount_a < 0 || amount_b < 0 {
        return Err(PairError::InsufficientInputAmount);
    }
    if amount_a == 0 && amount_b == 0 {
        return Err(PairError::InsufficientInputAmount);
    }

    // -----------------------------------------------------------------------
    // 2. Load state
    // -----------------------------------------------------------------------

    let mut state = get_pair_state(env).ok_or(PairError::NotInitialized)?;

    // Requested amounts must not exceed current reserves.
    if amount_a > state.reserve_a || amount_b > state.reserve_b {
        return Err(PairError::InsufficientLiquidity);
    }

    // Snapshot pre-loan constant-product invariant k = reserve_a * reserve_b.
    let pre_k = state
        .reserve_a
        .checked_mul(state.reserve_b)
        .ok_or(PairError::Overflow)?;

    // -----------------------------------------------------------------------
    // 3. Reentrancy guard — first state write
    // -----------------------------------------------------------------------

    // Acquiring the lock writes `locked = true` to instance storage.
    // On any subsequent Err return, Soroban rolls back ALL state (including
    // this write), so the lock is implicitly released on every error path.
    reentrancy::acquire(env)?;

    // -----------------------------------------------------------------------
    // 4. Fee calculation
    // -----------------------------------------------------------------------

    // Prefer the pool's configured baseline fee if it exceeds the flash floor.
    let pool_fee_bps = get_fee_state(env)
        .map(|fs| fs.baseline_fee_bps)
        .unwrap_or(FLASH_FEE_FLOOR_BPS);

    let fee_a = if amount_a > 0 {
        compute_flash_fee(amount_a, pool_fee_bps)
    } else {
        0
    };
    let fee_b = if amount_b > 0 {
        compute_flash_fee(amount_b, pool_fee_bps)
    } else {
        0
    };

    // -----------------------------------------------------------------------
    // 5. Transfer requested tokens to receiver
    // -----------------------------------------------------------------------

    let contract = env.current_contract_address();

    if amount_a > 0 {
        TokenClient::new(env, &state.token_a).transfer(&contract, receiver, &amount_a);
    }
    if amount_b > 0 {
        TokenClient::new(env, &state.token_b).transfer(&contract, receiver, &amount_b);
    }

    // -----------------------------------------------------------------------
    // 6. Invoke receiver callback
    // -----------------------------------------------------------------------

    // The receiver MUST repay `amount + fee` for each borrowed token before
    // `on_flash_loan` returns.  We pass the pair contract address as
    // `initiator` so the receiver knows the repayment destination.
    FlashReceiverClient::new(env, receiver).on_flash_loan(
        &contract,       // initiator = pair address (repayment destination)
        &state.token_a,
        &state.token_b,
        &amount_a,
        &amount_b,
        &fee_a,
        &fee_b,
        data,
    );

    // -----------------------------------------------------------------------
    // 7. Repayment verification
    // -----------------------------------------------------------------------

    let new_balance_a = TokenClient::new(env, &state.token_a).balance(&contract);
    let new_balance_b = TokenClient::new(env, &state.token_b).balance(&contract);

    // Each borrowed token's new balance must be >= old_reserve + fee.
    // Net effect: the pool gains exactly `fee` per token (or more).
    if amount_a > 0 {
        let required_a = state
            .reserve_a
            .checked_add(fee_a)
            .ok_or(PairError::Overflow)?;
        if new_balance_a < required_a {
            return Err(PairError::FlashLoanNotRepaid);
        }
    }
    if amount_b > 0 {
        let required_b = state
            .reserve_b
            .checked_add(fee_b)
            .ok_or(PairError::Overflow)?;
        if new_balance_b < required_b {
            return Err(PairError::FlashLoanNotRepaid);
        }
    }

    // -----------------------------------------------------------------------
    // 8. Reserve update
    // -----------------------------------------------------------------------

    state.reserve_a = new_balance_a;
    state.reserve_b = new_balance_b;

    // -----------------------------------------------------------------------
    // 9. k-invariant check
    // -----------------------------------------------------------------------

    // post_k must be >= pre_k; the fee income ensures this when repaid.
    let post_k = state
        .reserve_a
        .checked_mul(state.reserve_b)
        .ok_or(PairError::Overflow)?;

    if post_k < pre_k {
        return Err(PairError::InvalidK);
    }

    state.k_last = post_k;

    // -----------------------------------------------------------------------
    // 10. Persist updated reserves and emit event
    // -----------------------------------------------------------------------

    set_pair_state(env, &state);

    PairEvents::flash_loan(env, receiver, amount_a, amount_b, fee_a, fee_b);

    // -----------------------------------------------------------------------
    // 11. Release reentrancy lock
    // -----------------------------------------------------------------------

    reentrancy::release(env);

    Ok(())
}
