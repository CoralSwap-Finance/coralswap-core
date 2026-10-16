#![cfg(test)]

use soroban_sdk::testutils::Ledger as _;
use soroban_sdk::{Address, Env};

use crate::errors::OracleError;
use crate::oracle::{consult_twap, update_cumulative_prices, MAX_OBSERVATIONS, MAX_TWAP_WINDOW};
use crate::Pair;

fn setup_env() -> (Env, Address) {
    let env = Env::default();
    let contract_id = env.register(Pair, ());
    (env, contract_id)
}

#[test]
fn ring_buffer_capped_at_24() {
    let (env, contract_id) = setup_env();

    env.as_contract(&contract_id, || {
        // Push 30 observations
        for _i in 0..30 {
            update_cumulative_prices(&env, 100, 200, 1);
        }

        let state = crate::storage::get_oracle_state(&env);
        assert_eq!(state.len(), 24, "ring buffer must not exceed 24 entries");
        assert_eq!(MAX_OBSERVATIONS, 24);
    });
}

/// Issue #308: once the ring is full, the retained window is the newest
/// `MAX_OBSERVATIONS` samples — in order — and the backing store never grows
/// past capacity.
///
/// Ordering across the wrap is the property a ring buffer gets wrong if the
/// indices are advanced inconsistently: the oldest live sample has to be at
/// `head`, not at physical slot 0, and the newest has to follow it.
#[test]
fn ring_retains_the_newest_capacity_samples_in_order() {
    let (env, contract_id) = setup_env();

    env.as_contract(&contract_id, || {
        // 30 samples, one ledger apart, well inside the TWAP window so nothing
        // is dropped by time and the count cap is the only thing evicting.
        let first = 1_000u32;
        for i in 0..30u32 {
            env.ledger().set_sequence_number(first + i);
            update_cumulative_prices(&env, 100, 200, 1);
        }

        let state = crate::storage::get_oracle_state(&env);
        assert_eq!(state.len(), MAX_OBSERVATIONS);
        assert_eq!(
            state.slots.len(),
            MAX_OBSERVATIONS,
            "the backing store must stay at capacity, not grow per update"
        );

        // Samples 6..29 survive; sample 0..5 were evicted.
        for i in 0..MAX_OBSERVATIONS {
            let (ledger, _, _) = state.sample(i).unwrap();
            assert_eq!(
                ledger,
                (first + 6 + i) as u64,
                "sample {i} is out of order after the ring wrapped"
            );
        }
        assert_eq!(state.oldest().unwrap().0, (first + 6) as u64);
        assert_eq!(state.newest().unwrap().0, (first + 29) as u64);
        assert_eq!(state.sample(MAX_OBSERVATIONS), None);
    });
}

/// Issue #308, second half: eviction is driven by elapsed ledgers, not by how
/// many samples the ring happens to hold.
///
/// A quiet pool never accumulates 24 samples, so count-based pruning kept its
/// very first observation forever and a later window query could interpolate
/// across a gap of thousands of ledgers.
#[test]
fn stale_samples_are_dropped_by_time_not_count() {
    let (env, contract_id) = setup_env();

    env.as_contract(&contract_id, || {
        env.ledger().set_sequence_number(1_000);
        update_cumulative_prices(&env, 100, 200, 1);

        // One ledger past a full window: the first sample is now unreachable
        // for any accepted window, even though the ring is nowhere near full.
        env.ledger().set_sequence_number(1_000 + MAX_TWAP_WINDOW + 1);
        update_cumulative_prices(&env, 100, 200, MAX_TWAP_WINDOW as u64 + 1);

        let state = crate::storage::get_oracle_state(&env);
        assert_eq!(
            state.len(),
            1,
            "a sample older than the TWAP window must be dropped even though the ring is not full"
        );
        assert_eq!(state.oldest().unwrap().0, (1_000 + MAX_TWAP_WINDOW + 1) as u64);
    });
}

/// The time-based cutoff has to keep the sample that is exactly one window old:
/// `consult_twap(MAX_TWAP_WINDOW)` legitimately needs it as its start point.
#[test]
fn samples_exactly_one_window_old_are_kept() {
    let (env, contract_id) = setup_env();

    env.as_contract(&contract_id, || {
        env.ledger().set_sequence_number(1_000);
        update_cumulative_prices(&env, 100, 200, 1);

        env.ledger().set_sequence_number(1_000 + MAX_TWAP_WINDOW);
        update_cumulative_prices(&env, 100, 200, MAX_TWAP_WINDOW as u64);

        let state = crate::storage::get_oracle_state(&env);
        assert_eq!(state.len(), 2, "a sample exactly one window old is still usable");
        assert_eq!(state.sample(0).unwrap().0, 1_000);
    });
}

/// Pruning advances `head` and leaves the dead slots in place, so a ring that
/// empties out and refills reuses its storage instead of re-appending, and the
/// order is intact after the slot reuse.
#[test]
fn a_pruned_ring_reuses_its_freed_slots_in_place() {
    let (env, contract_id) = setup_env();

    env.as_contract(&contract_id, || {
        let first = 1_000u32;
        for i in 0..MAX_OBSERVATIONS {
            env.ledger().set_sequence_number(first + i);
            update_cumulative_prices(&env, 100, 200, 1);
        }
        // Jump past the window: everything above is now stale.
        let after_gap = first + MAX_OBSERVATIONS + MAX_TWAP_WINDOW + 1;
        env.ledger().set_sequence_number(after_gap);
        update_cumulative_prices(&env, 100, 200, 1);

        let state = crate::storage::get_oracle_state(&env);
        assert_eq!(state.len(), 1);
        assert_eq!(
            state.slots.len(),
            MAX_OBSERVATIONS,
            "dead slots must be reused in place rather than compacted or re-appended"
        );

        // Refill within the window; the ring must fill back up and stay ordered.
        for i in 1..MAX_OBSERVATIONS {
            env.ledger().set_sequence_number(after_gap + i);
            update_cumulative_prices(&env, 100, 200, 1);
        }
        let refilled = crate::storage::get_oracle_state(&env);
        assert_eq!(refilled.len(), MAX_OBSERVATIONS);
        assert_eq!(refilled.slots.len(), MAX_OBSERVATIONS);
        for i in 0..MAX_OBSERVATIONS {
            assert_eq!(
                refilled.sample(i).unwrap().0,
                (after_gap + i) as u64,
                "sample {i} is out of order after a prune refilled the ring"
            );
        }
    });
}

/// The insertion that has to evict must not cost more than the one that simply
/// appends, which is what `Vec::remove(0)` could not promise: it moved every
/// live sample down one slot on each eviction, and the host charges for that
/// movement.
///
/// Measured with the host's own CPU meter, so the comparison is against real
/// resource consumption rather than a proxy. Comparing the append that fills the
/// ring with the evicting insert that follows it isolates the two strategies:
/// both write one storage entry, and the only difference is how the buffer makes
/// room.
///
/// `EVICT_INSERT_SLACK` is 5% because that is what separates the two
/// implementations with room on both sides, measured on this commit with
/// `soroban-sdk 27.0.5`: the in-place ring's evicting insert costs 14,642 CPU
/// instructions against 14,721 for the appending one (0.5% *cheaper*), while the
/// previous `remove(0)` strategy cost 16,157 against 14,843 (+8.9%). The slack
/// covers the incidental asymmetry — the evicting call writes one more entry to
/// storage — without covering the shift.
#[test]
fn an_evicting_insert_does_not_cost_more_than_an_appending_one() {
    /// Percentage of extra CPU instructions the evicting insert may cost.
    const EVICT_INSERT_SLACK: u64 = 5;

    let (env, contract_id) = setup_env();

    env.as_contract(&contract_id, || {
        let first = 1_000u32;

        // Fill to one below capacity, then measure the append that fills it.
        for i in 0..(MAX_OBSERVATIONS - 1) {
            env.ledger().set_sequence_number(first + i);
            update_cumulative_prices(&env, 100, 200, 1);
        }
        env.ledger().set_sequence_number(first + MAX_OBSERVATIONS - 1);
        let append = measure_cpu(&env, || update_cumulative_prices(&env, 100, 200, 1));

        // Ring is full: the next insert has to evict.
        env.ledger().set_sequence_number(first + MAX_OBSERVATIONS);
        let evict = measure_cpu(&env, || update_cumulative_prices(&env, 100, 200, 1));

        assert!(
            evict * 100 <= append * (100 + EVICT_INSERT_SLACK),
            "an evicting insert ({evict} CPU instructions) must not cost more than \
             an appending one ({append}) by more than {EVICT_INSERT_SLACK}%; it does \
             when the ring shifts its samples instead of overwriting the oldest slot"
        );
    });
}

/// CPU instructions charged by the host for one call, measured in isolation.
fn measure_cpu<T, F: FnOnce() -> T>(env: &Env, f: F) -> u64 {
    env.cost_estimate().budget().reset_unlimited();
    let before = env.cost_estimate().budget().cpu_instruction_cost();
    let out = f();
    drop(out);
    env.cost_estimate().budget().cpu_instruction_cost().saturating_sub(before)
}

#[test]
fn observations_are_appended_on_price_update() {
    let (env, contract_id) = setup_env();

    env.as_contract(&contract_id, || {
        update_cumulative_prices(&env, 100, 200, 10);

        let state = crate::storage::get_oracle_state(&env);
        // The accumulators live on the oracle state (issue #312) rather than on
        // PairStorage, where they used to sit unwritten.
        //
        // `price_a` is `reserve_b / reserve_a * elapsed` = 200/100 * 10 = 20.
        // `price_b` is `reserve_a / reserve_b * elapsed` = 100/200 * 10, and
        // that division truncates to 0 — the same integer-division truncation
        // Uniswap V2's accumulators inherit.
        assert_eq!(state.price_a_cumulative, 20);
        assert_eq!(state.price_b_cumulative, 0);
        assert_eq!(state.len(), 1);
        let (_, cum_a, cum_b) = state.sample(0).unwrap();
        assert_eq!(cum_a, 20);
        assert_eq!(cum_b, 0);
    });
}

/// The accumulators must compound across calls, which is the whole point of a
/// cumulative price: a second sample adds to the first rather than replacing it.
#[test]
fn cumulative_prices_accumulate_across_calls() {
    let (env, contract_id) = setup_env();

    env.as_contract(&contract_id, || {
        update_cumulative_prices(&env, 100, 200, 10);
        let first = crate::storage::get_oracle_state(&env);
        assert_eq!(first.price_a_cumulative, 20);

        update_cumulative_prices(&env, 100, 200, 5);
        let second = crate::storage::get_oracle_state(&env);
        assert_eq!(second.price_a_cumulative, 30, "accumulators must compound");
        assert_eq!(second.len(), 2);

        // Every sample in the buffer must be non-decreasing.
        let (_, prev, _) = second.sample(0).unwrap();
        let (_, latest, _) = second.sample(1).unwrap();
        assert!(latest >= prev);
    });
}

/// Dropping the out-parameters means the state is the single source of truth:
/// a caller cannot update the buffer while discarding the accumulated totals.
#[test]
fn accumulator_and_buffer_cannot_diverge() {
    let (env, contract_id) = setup_env();

    env.as_contract(&contract_id, || {
        for elapsed in [1i128, 2, 3] {
            update_cumulative_prices(&env, 100, 200, elapsed as u64);
        }

        let state = crate::storage::get_oracle_state(&env);
        let (_, latest_a, latest_b) = state.newest().unwrap();
        assert_eq!(state.price_a_cumulative, latest_a);
        assert_eq!(state.price_b_cumulative, latest_b);
    });
}

/// A zero reserve makes the price undefined, so the update must be a no-op
/// rather than a division-by-zero wrap.
#[test]
fn update_is_noop_when_reserves_are_empty() {
    let (env, contract_id) = setup_env();

    env.as_contract(&contract_id, || {
        update_cumulative_prices(&env, 0, 200, 10);
        let state = crate::storage::get_oracle_state(&env);
        assert_eq!(state.price_a_cumulative, 0);
        assert_eq!(state.len(), 0);
    });
}

/// Zero elapsed time contributes nothing and must not create a sample.
#[test]
fn update_is_noop_when_no_time_has_elapsed() {
    let (env, contract_id) = setup_env();

    env.as_contract(&contract_id, || {
        update_cumulative_prices(&env, 100, 200, 0);
        let state = crate::storage::get_oracle_state(&env);
        assert_eq!(state.price_a_cumulative, 0);
        assert_eq!(state.len(), 0);
    });
}

#[test]
fn consult_twap_window_zero_returns_error() {
    let (env, contract_id) = setup_env();

    env.as_contract(&contract_id, || {
        let result = consult_twap(&env, 0);
        assert_eq!(result, Err(OracleError::WindowTooShort));
    });
}

#[test]
fn consult_twap_window_too_long_returns_error() {
    let (env, contract_id) = setup_env();

    env.as_contract(&contract_id, || {
        let result = consult_twap(&env, MAX_TWAP_WINDOW + 1);
        assert_eq!(result, Err(OracleError::WindowTooLong));
    });
}

#[test]
fn consult_twap_no_observations_returns_error() {
    let (env, contract_id) = setup_env();

    env.as_contract(&contract_id, || {
        let result = consult_twap(&env, 100);
        assert_eq!(result, Err(OracleError::WindowTooShort));
    });
}

#[test]
fn consult_twap_boundary_exact_window() {
    let (env, contract_id) = setup_env();

    env.as_contract(&contract_id, || {
        // Observation 1 at sequence 100
        env.ledger().set_sequence_number(100);
        update_cumulative_prices(&env, 100, 200, 10);

        // Observation 2 at sequence 200 (exact window of 100 ledgers)
        env.ledger().set_sequence_number(200);
        update_cumulative_prices(&env, 100, 200, 100);

        let result = consult_twap(&env, 100);
        assert!(result.is_ok(), "exact window boundary must succeed");
        let (avg_a, avg_b) = result.unwrap();
        assert!(avg_a > 0 || avg_b > 0);
    });
}

#[test]
fn consult_twap_boundary_window_plus_one() {
    let (env, contract_id) = setup_env();

    env.as_contract(&contract_id, || {
        // Observation 1 at sequence 100
        env.ledger().set_sequence_number(100);
        update_cumulative_prices(&env, 100, 200, 10);

        // Observation 2 at sequence 201 (window + 1 for window_ledgers = 100)
        env.ledger().set_sequence_number(201);
        update_cumulative_prices(&env, 100, 200, 101);

        let result = consult_twap(&env, 100);
        assert!(result.is_ok(), "window + 1 boundary must succeed");
    });
}

#[test]
fn consult_twap_boundary_insufficient_window() {
    let (env, contract_id) = setup_env();

    env.as_contract(&contract_id, || {
        // Observation 1 at sequence 100
        env.ledger().set_sequence_number(100);
        update_cumulative_prices(&env, 100, 200, 10);

        // Observation 2 at sequence 150
        env.ledger().set_sequence_number(150);
        update_cumulative_prices(&env, 100, 200, 50);

        // Advance ledger to 200 without updating observation (latest obs is at 150 < target 100 + window 100 = 200)
        env.ledger().set_sequence_number(200);

        let result = consult_twap(&env, 100);
        assert_eq!(
            result,
            Err(OracleError::WindowTooShort),
            "insufficient window must return WindowTooShort"
        );

        // Also test when oldest observation is newer than target:
        // Window 120 -> target = 200 - 120 = 80, but oldest obs is 100 > 80
        let result_too_short = consult_twap(&env, 120);
        assert_eq!(
            result_too_short,
            Err(OracleError::WindowTooShort),
            "window older than oldest observation must return WindowTooShort"
        );
    });
}
