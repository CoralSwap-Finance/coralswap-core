use soroban_sdk::Env;
use crate::storage::FeeState;

/// Maximum decay window in ledger blocks (~24h at 5s blocks).
const MAX_DECAY_WINDOW: u64 = 17_280;

/// Applies time-based decay to a stale fee accumulator.
/// Called before every swap to prevent idle pools from charging inflated fees.
pub fn apply_time_decay(_env: &Env, fee_state: &mut FeeState, current_ledger: u64) {
    let elapsed = current_ledger.saturating_sub(fee_state.last_fee_update);

    if elapsed == 0 || fee_state.vol_accumulator == 0 {
        return;
    }

    // Clamp decay window so we never completely overshoot.
    let decay_elapsed = elapsed.min(MAX_DECAY_WINDOW);
    let decay_window = fee_state
        .decay_threshold_blocks
        .max(1)
        .max(MAX_DECAY_WINDOW);

    // Linear decay: reduce vol_accumulator proportionally to time elapsed.
    // new_vol = old_vol * (1 - elapsed / decay_window)
    let remaining = MAX_DECAY_WINDOW.saturating_sub(decay_elapsed) as i128;
    let window = decay_window as i128;

    fee_state.vol_accumulator = (fee_state.vol_accumulator * remaining) / window;
    fee_state.last_fee_update = current_ledger;
}
