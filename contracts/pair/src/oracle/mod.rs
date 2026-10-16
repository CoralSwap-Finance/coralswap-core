use crate::errors::OracleError;
use crate::storage::{get_oracle_state, set_oracle_state, OracleState};
use soroban_sdk::Env;

pub const MAX_TWAP_WINDOW: u32 = 86400;

// The accumulator, its sample buffer and the constant bounding that buffer are
// only reachable from tests: `update_cumulative_prices` is not yet called by any
// pair entry point, which is tracked as #271. They are kept compiling and
// tested here so that #271 is a wiring change rather than a rewrite, and so the
// dead storage they used to occupy is not simply re-added later.
//
// Issue #312 removed the unwritten `price_a_cumulative` / `price_b_cumulative`
// fields from `PairStorage`; these are the replacement home for them.
#[allow(dead_code)]
pub const MAX_OBSERVATIONS: u32 = 24;

// ---------------------------------------------------------------------------
// Ring buffer
// ---------------------------------------------------------------------------
//
// The sample buffer is a fixed-capacity ring (issue #308). `OracleState::slots`
// holds the physical slots and `OracleState::head` / `OracleState::count`
// describe which of them are live, so that appending a sample and evicting the
// oldest are both single host calls. The previous implementation called
// `Vec::remove(0)` — which moves every remaining sample — on every update once
// the buffer was full. Use the accessors below instead of touching `slots`.

impl OracleState {
    /// Number of live samples.
    pub fn len(&self) -> u32 {
        self.count
    }

    /// Whether the buffer holds no samples at all.
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// The `index`-th live sample, oldest first, mapped onto physical slots.
    ///
    /// Returns `None` for `index >= self.len()`.
    pub fn sample(&self, index: u32) -> Option<(u64, i128, i128)> {
        if index >= self.count {
            return None;
        }
        self.slots.get((self.head + index) % MAX_OBSERVATIONS)
    }

    /// The oldest live sample, or `None` when the buffer is empty.
    pub fn oldest(&self) -> Option<(u64, i128, i128)> {
        self.sample(0)
    }

    /// The most recent sample, or `None` when the buffer is empty.
    pub fn newest(&self) -> Option<(u64, i128, i128)> {
        match self.count {
            0 => None,
            count => self.sample(count - 1),
        }
    }

    /// Appends `sample`, evicting the oldest one once the ring is full.
    ///
    /// Either branch is a single host call on `slots`: an append while there is
    /// room, a `set` in place of the oldest slot once there is not. Nothing is
    /// moved, so the cost does not grow with how full the ring is.
    fn push(&mut self, sample: (u64, i128, i128)) {
        if self.count < MAX_OBSERVATIONS {
            // One past the newest live sample. That slot is either a dead one
            // left behind by pruning (still present in `slots`) or the next
            // free slot, which is where an append belongs.
            let index = (self.head + self.count) % MAX_OBSERVATIONS;
            if index < self.slots.len() {
                self.slots.set(index, sample);
            } else {
                self.slots.push_back(sample);
            }
            self.count += 1;
        } else {
            // Full: the oldest live sample sits at `head`, so overwriting it
            // and advancing `head` is both the eviction and the append.
            self.slots.set(self.head, sample);
            self.head = (self.head + 1) % MAX_OBSERVATIONS;
        }
    }

    /// Drops the samples that no `consult_twap` window can still reach.
    ///
    /// A sample is reachable only while `now - ledger <= MAX_TWAP_WINDOW`:
    /// `consult_twap` rejects any window longer than that, so an older sample
    /// could never be the start of a returned average. Dropping it is what
    /// stops stale observations polluting a window query, which is the half of
    /// issue #308 that pruning by count could not do — a quiet pool never
    /// reaches 24 samples, so count-based pruning kept its first sample forever.
    ///
    /// This advances `head` rather than removing from the front of `slots`: the
    /// dead slots stay where they are so that the next push can reuse them
    /// without moving anything.
    fn prune_expired(&mut self, now: u64) {
        let window = MAX_TWAP_WINDOW as u64;
        while let Some((ledger, _, _)) = self.oldest() {
            if ledger + window >= now {
                break;
            }
            self.head = (self.head + 1) % MAX_OBSERVATIONS;
            self.count -= 1;
        }
    }
}

/// Advances the cumulative-price accumulators and records a sample.
///
/// The accumulators are owned by [`OracleState`] (issue #312) rather than by
/// `PairStorage`, so this function no longer takes mutable out-parameters: the
/// previous pair always holds the authoritative value, which removed the
/// possibility of a caller silently dropping an update on the floor.
///
/// This is still not wired into any pair entry point — that is #271. Until then
/// it is reachable only from tests, and the accumulators it maintains are
/// honestly scoped to the oracle rather than advertised as pool state.
///
/// No-op when either reserve is zero (the price is undefined) or when no time
/// has elapsed.
#[allow(dead_code)]
pub fn update_cumulative_prices(env: &Env, reserve_a: i128, reserve_b: i128, time_elapsed: u64) {
    if reserve_a == 0 || reserve_b == 0 || time_elapsed == 0 {
        return;
    }

    let price_a_delta = (reserve_b / reserve_a).wrapping_mul(time_elapsed as i128);
    let price_b_delta = (reserve_a / reserve_b).wrapping_mul(time_elapsed as i128);

    let mut oracle_state = get_oracle_state(env);
    oracle_state.price_a_cumulative = oracle_state.price_a_cumulative.wrapping_add(price_a_delta);
    oracle_state.price_b_cumulative = oracle_state.price_b_cumulative.wrapping_add(price_b_delta);

    let now = env.ledger().sequence() as u64;
    oracle_state.prune_expired(now);
    oracle_state.push((now, oracle_state.price_a_cumulative, oracle_state.price_b_cumulative));
    set_oracle_state(env, &oracle_state);
}

pub fn consult_twap(env: &Env, window_ledgers: u32) -> Result<(i128, i128), OracleError> {
    if window_ledgers == 0 {
        return Err(OracleError::WindowTooShort);
    }
    if window_ledgers > MAX_TWAP_WINDOW {
        return Err(OracleError::WindowTooLong);
    }

    let oracle_state = get_oracle_state(env);
    if oracle_state.is_empty() {
        return Err(OracleError::WindowTooShort);
    }

    let current_ledger = env.ledger().sequence() as u64;
    let target = current_ledger.saturating_sub(window_ledgers as u64);

    let len = oracle_state.len();

    // If the oldest observation is newer than our target, the window is too short.
    let (oldest_ledger, oldest_a, oldest_b) = oracle_state.sample(0).unwrap();
    if oldest_ledger > target {
        return Err(OracleError::WindowTooShort);
    }

    let (mut start_ledger, mut start_a, mut start_b) = (oldest_ledger, oldest_a, oldest_b);
    let (mut end_ledger, mut end_a, mut end_b) = (oldest_ledger, oldest_a, oldest_b);

    for i in 0..len {
        let (l, a, b) = oracle_state.sample(i).unwrap();
        if l <= target {
            start_ledger = l;
            start_a = a;
            start_b = b;
        }
        if l >= target && end_ledger <= target {
            end_ledger = l;
            end_a = a;
            end_b = b;
            break;
        }
    }

    // Interpolate
    let interpolated_a = if end_ledger == start_ledger {
        start_a
    } else {
        start_a.wrapping_add(
            (end_a.wrapping_sub(start_a)) * (target as i128 - start_ledger as i128)
                / (end_ledger as i128 - start_ledger as i128),
        )
    };

    let interpolated_b = if end_ledger == start_ledger {
        start_b
    } else {
        start_b.wrapping_add(
            (end_b.wrapping_sub(start_b)) * (target as i128 - start_ledger as i128)
                / (end_ledger as i128 - start_ledger as i128),
        )
    };

    // A TWAP is the change in cumulative price over the change in time, so the
    // most recent sample is the "now" endpoint; only the start needs
    // interpolation against the requested target ledger.
    let (latest_ledger, latest_a, latest_b) = oracle_state.newest().unwrap();

    if latest_ledger < target + window_ledgers as u64 {
        return Err(OracleError::WindowTooShort);
    }
    let time_elapsed = latest_ledger.saturating_sub(target);
    if time_elapsed == 0 {
        return Ok((0, 0));
    }

    let delta_a = latest_a.wrapping_sub(interpolated_a);
    let delta_b = latest_b.wrapping_sub(interpolated_b);

    let price_a_avg = delta_a / (time_elapsed as i128);
    let price_b_avg = delta_b / (time_elapsed as i128);

    Ok((price_a_avg, price_b_avg))
}
