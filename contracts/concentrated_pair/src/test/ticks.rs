use super::{setup_env, setup_initialized};
use crate::errors::ConcentratedPairError;
use crate::storage::{get_tick, set_tick, TickInfo};

#[test]
fn tick_entries_round_trip_and_reclaim() {
    let (env, contract_id, _, _, _) = setup_env();
    let client = setup_initialized(&env, &contract_id);

    let info = TickInfo { liquidity_gross: 500, liquidity_net: -500 };
    env.as_contract(&contract_id, || {
        set_tick(&env, -60, &info).unwrap();
        // Storage helper and contract view must agree.
        assert_eq!(get_tick(&env, -60), Some(info.clone()));
    });
    assert_eq!(client.get_tick(&-60), Some(info));

    // Neighbouring ticks are independent entries.
    assert_eq!(client.get_tick(&60), None);
    assert_eq!(client.get_tick(&-59), None);

    // Zeroing gross liquidity reclaims the entry entirely.
    env.as_contract(&contract_id, || {
        set_tick(&env, -60, &TickInfo { liquidity_gross: 0, liquidity_net: 0 }).unwrap();
    });
    assert_eq!(client.get_tick(&-60), None);
}

#[test]
fn zero_gross_with_nonzero_net_is_rejected() {
    let (env, contract_id, _, _, _) = setup_env();
    let _client = setup_initialized(&env, &contract_id);

    env.as_contract(&contract_id, || {
        let result = set_tick(&env, 0, &TickInfo { liquidity_gross: 0, liquidity_net: 7 });
        assert_eq!(result, Err(ConcentratedPairError::InvalidInput));
    });
}

#[test]
fn reclaiming_an_absent_tick_is_a_no_op() {
    let (env, contract_id, _, _, _) = setup_env();
    let _client = setup_initialized(&env, &contract_id);

    env.as_contract(&contract_id, || {
        set_tick(&env, 120, &TickInfo { liquidity_gross: 0, liquidity_net: 0 }).unwrap();
        assert_eq!(get_tick(&env, 120), None);
    });
}
