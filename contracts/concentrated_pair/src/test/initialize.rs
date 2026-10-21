use soroban_sdk::{
    testutils::{Address as _, Events as _},
    Address,
};

use super::{setup_env, setup_initialized, SQRT_PRICE_ONE};
use crate::errors::ConcentratedPairError;
use crate::tick_math::{MAX_SQRT_RATIO, MIN_SQRT_RATIO};
use crate::ConcentratedPairClient;

#[test]
fn happy_path_initializes_all_state() {
    let (env, contract_id, factory, token_a, token_b) = setup_env();
    let client = ConcentratedPairClient::new(&env, &contract_id);

    client.initialize(&factory, &token_a, &token_b, &30, &60, &SQRT_PRICE_ONE);

    let slot0 = client.get_slot0();
    assert_eq!(slot0.sqrt_price, SQRT_PRICE_ONE);
    assert_eq!(slot0.tick, 0);
    assert_eq!(slot0.fee_protocol, 0);

    let config = client.get_config();
    assert_eq!(config.factory, factory);
    assert_eq!(config.token_a, token_a);
    assert_eq!(config.token_b, token_b);
    assert_eq!(config.fee_bps, 30);
    assert_eq!(config.tick_spacing, 60);

    // A fresh pool holds no active liquidity and no initialized ticks.
    assert_eq!(client.get_liquidity(), 0);
    assert_eq!(client.get_tick(&0), None);
    assert_eq!(client.get_tick(&-60), None);
}

#[test]
fn initial_tick_is_derived_from_the_price() {
    let (env, contract_id, factory, token_a, token_b) = setup_env();
    let client = ConcentratedPairClient::new(&env, &contract_id);

    // √2·2^64: price 2.0 sits inside tick 6931 (log_1.0001(2) = 6931.56…).
    let sqrt_price_2 = 26_087_635_650_665_564_424u128;
    client.initialize(&factory, &token_a, &token_b, &30, &60, &sqrt_price_2);

    let slot0 = client.get_slot0();
    assert_eq!(slot0.sqrt_price, sqrt_price_2);
    assert_eq!(slot0.tick, 6_931);
}

#[test]
fn double_initialize_is_rejected() {
    let (env, contract_id, factory, token_a, token_b) = setup_env();
    let client = ConcentratedPairClient::new(&env, &contract_id);

    client.initialize(&factory, &token_a, &token_b, &30, &60, &SQRT_PRICE_ONE);
    let result = client.try_initialize(&factory, &token_a, &token_b, &30, &60, &SQRT_PRICE_ONE);
    assert_eq!(result, Err(Ok(ConcentratedPairError::AlreadyInitialized)));
}

#[test]
fn identical_and_self_referential_tokens_are_rejected() {
    let (env, contract_id, factory, token_a, token_b) = setup_env();
    let client = ConcentratedPairClient::new(&env, &contract_id);

    // token_a == token_b
    let result = client.try_initialize(&factory, &token_a, &token_a, &30, &60, &SQRT_PRICE_ONE);
    assert_eq!(result, Err(Ok(ConcentratedPairError::InvalidInput)));

    // a token that is the factory
    let result = client.try_initialize(&factory, &factory, &token_b, &30, &60, &SQRT_PRICE_ONE);
    assert_eq!(result, Err(Ok(ConcentratedPairError::InvalidInput)));
    let result = client.try_initialize(&factory, &token_a, &factory, &30, &60, &SQRT_PRICE_ONE);
    assert_eq!(result, Err(Ok(ConcentratedPairError::InvalidInput)));

    // a token that is the pool itself
    let result = client.try_initialize(&factory, &contract_id, &token_b, &30, &60, &SQRT_PRICE_ONE);
    assert_eq!(result, Err(Ok(ConcentratedPairError::InvalidInput)));
    let result = client.try_initialize(&factory, &token_a, &contract_id, &30, &60, &SQRT_PRICE_ONE);
    assert_eq!(result, Err(Ok(ConcentratedPairError::InvalidInput)));
}

#[test]
fn fee_and_tick_spacing_bounds_are_enforced() {
    let (env, contract_id, factory, token_a, token_b) = setup_env();
    let client = ConcentratedPairClient::new(&env, &contract_id);

    for (fee_bps, tick_spacing) in [
        (0u32, 60u32),                     // zero fee
        (crate::MAX_FEE_BPS + 1, 60),      // fee above cap
        (30, 0),                           // zero spacing
        (30, crate::MAX_TICK_SPACING + 1), // spacing above cap
    ] {
        let result = client.try_initialize(
            &factory,
            &token_a,
            &token_b,
            &fee_bps,
            &tick_spacing,
            &SQRT_PRICE_ONE,
        );
        assert_eq!(
            result,
            Err(Ok(ConcentratedPairError::InvalidInput)),
            "accepted fee_bps={fee_bps} tick_spacing={tick_spacing}"
        );
    }

    // The caps themselves are valid.
    client.initialize(
        &factory,
        &token_a,
        &token_b,
        &crate::MAX_FEE_BPS,
        &crate::MAX_TICK_SPACING,
        &SQRT_PRICE_ONE,
    );
}

#[test]
fn out_of_range_sqrt_prices_are_rejected_and_bounds_accepted() {
    let (env, contract_id, factory, token_a, token_b) = setup_env();
    let client = ConcentratedPairClient::new(&env, &contract_id);

    for bad in [0u128, MIN_SQRT_RATIO - 1, MAX_SQRT_RATIO, u128::MAX] {
        let result = client.try_initialize(&factory, &token_a, &token_b, &30, &60, &bad);
        assert_eq!(
            result,
            Err(Ok(ConcentratedPairError::InvalidSqrtPrice)),
            "accepted sqrt_price={bad}"
        );
    }

    // The minimum √price itself is valid and lands on MIN_TICK.
    client.initialize(&factory, &token_a, &token_b, &30, &60, &MIN_SQRT_RATIO);
    assert_eq!(client.get_slot0().tick, crate::tick_math::MIN_TICK);
}

#[test]
fn views_error_before_initialization() {
    let (env, contract_id, _, _, _) = setup_env();
    let client = ConcentratedPairClient::new(&env, &contract_id);

    assert_eq!(client.try_get_slot0(), Err(Ok(ConcentratedPairError::NotInitialized)));
    assert_eq!(client.try_get_config(), Err(Ok(ConcentratedPairError::NotInitialized)));
    assert_eq!(client.try_get_liquidity(), Err(Ok(ConcentratedPairError::NotInitialized)));
    assert_eq!(client.try_get_tick(&0), Err(Ok(ConcentratedPairError::NotInitialized)));
}

#[test]
fn get_tick_rejects_out_of_range_ticks() {
    let (env, contract_id, _, _, _) = setup_env();
    let client = setup_initialized(&env, &contract_id);

    assert_eq!(
        client.try_get_tick(&(crate::tick_math::MAX_TICK + 1)),
        Err(Ok(ConcentratedPairError::InvalidTick))
    );
    assert_eq!(
        client.try_get_tick(&(crate::tick_math::MIN_TICK - 1)),
        Err(Ok(ConcentratedPairError::InvalidTick))
    );
}

#[test]
fn initialize_emits_the_init_event() {
    let (env, contract_id, factory, token_a, token_b) = setup_env();
    let client = ConcentratedPairClient::new(&env, &contract_id);

    client.initialize(&factory, &token_a, &token_b, &30, &60, &SQRT_PRICE_ONE);

    let events = env.events().all().filter_by_contract(&contract_id);
    assert_eq!(events.events().len(), 1, "initialize must emit exactly one event");
}

#[test]
fn initialize_works_from_a_fresh_deploy() {
    // The "contract deploys successfully" acceptance criterion: register a
    // second instance of the compiled contract and drive it end to end.
    let (env, _, factory, token_a, token_b) = setup_env();
    let second = env.register(crate::ConcentratedPair, ());
    let client = ConcentratedPairClient::new(&env, &second);
    client.initialize(&factory, &token_a, &token_b, &5, &1, &SQRT_PRICE_ONE);
    assert_eq!(client.get_slot0().tick, 0);
}

#[test]
fn distinct_factory_lookalike_is_fine() {
    // A token merely *equal in value* to another address is caught; distinct
    // addresses pass. Regression guard for accidental over-restriction.
    let (env, contract_id, factory, token_a, _) = setup_env();
    let client = ConcentratedPairClient::new(&env, &contract_id);
    let token_c = Address::generate(&env);
    client.initialize(&factory, &token_a, &token_c, &30, &60, &SQRT_PRICE_ONE);
}
