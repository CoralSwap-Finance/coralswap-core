use crate::storage::FeeState;
use soroban_sdk::Env;

/// Maximum number of decay iterations to prevent excessive computation
/// in case of extremely long idle periods.
const MAX_DECAY_PERIODS: u64 = 64;

/// Applies exponential time-based decay to a stale fee accumulator.
///
/// Called before every swap to prevent idle pools from charging inflated fees.
/// Each decay period divides `vol_accumulator` by `cooldown_divisor`, producing
/// compounding exponential decay towards zero.
///
/// # Decay math
///
/// ```text
/// elapsed       = current_ledger - last_fee_update
/// decay_periods = elapsed / decay_threshold_blocks
/// vol_new       = vol_old / cooldown_divisor ^ decay_periods
/// ```
///
/// We apply iterative division rather than computing a power to avoid
/// overflow from exponentiation, and cap iterations at [`MAX_DECAY_PERIODS`]
/// to bound gas costs.
pub fn apply_time_decay(_env: &Env, fee_state: &mut FeeState, current_ledger: u64) {
    let elapsed = current_ledger.saturating_sub(fee_state.last_fee_update);

    // Nothing to do if no time passed or accumulator is already zero.
    if elapsed == 0 || fee_state.vol_accumulator <= 0 {
        return;
    }

    // Guard: cooldown_divisor must be >= 2 for meaningful decay.
    // A divisor of 0 or 1 would loop forever or never decay.
    let divisor = (fee_state.cooldown_divisor as u64).max(2) as i128;

    // Number of full decay periods elapsed.
    let threshold = fee_state.decay_threshold_blocks.max(1);
    let decay_periods = (elapsed / threshold).min(MAX_DECAY_PERIODS);

    // Apply compounding exponential decay: vol /= divisor per period.
    for _ in 0..decay_periods {
        fee_state.vol_accumulator /= divisor;

        // Early exit: once accumulator hits zero, no further division changes it.
        if fee_state.vol_accumulator <= 0 {
            fee_state.vol_accumulator = 0;
            break;
        }
    }

    // Floor clamp — defensive, should already be >= 0 from division of positive values.
    if fee_state.vol_accumulator < 0 {
        fee_state.vol_accumulator = 0;
    }

    // Update ledger sequence to prevent re-decay on the same block.
    fee_state.last_fee_update = current_ledger;
}
