use crate::errors::PairError;
use crate::storage::{get_fee_state, get_pair_state, get_reentrancy_guard};
use crate::{Pair, PairClient};
use soroban_sdk::{testutils::Address as _, Address, Env};

fn setup_env() -> (Env, Address, Address, Address, Address, Address) {
    let env = Env::default();
    let contract_id = env.register(Pair, ());
    let factory = Address::generate(&env);
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);
    let lp_token = Address::generate(&env);
    (env, contract_id, factory, token_a, token_b, lp_token)
}

#[test]
fn happy_path_initializes_all_state() {
    let (env, contract_id, factory, token_a, token_b, lp_token) = setup_env();
    let client = PairClient::new(&env, &contract_id);

    client.initialize(&factory, &token_a, &token_b, &lp_token);

    // Reserves should be (0, 0, _)
    let (r_a, r_b, _ts) = client.get_reserves();
    assert_eq!(r_a, 0);
    assert_eq!(r_b, 0);

    // FeeState should be initialized
    env.as_contract(&contract_id, || {
        let fee = get_fee_state(&env).expect("FeeState missing");
        assert_eq!(fee.baseline_fee_bps, 30);
        assert_eq!(fee.min_fee_bps, 10);
        assert_eq!(fee.max_fee_bps, 100);
        assert_eq!(fee.vol_accumulator, 0);
    });

    // ReentrancyGuard should be unlocked
    env.as_contract(&contract_id, || {
        let guard = get_reentrancy_guard(&env);
        assert!(!guard.locked);
    });
}

#[test]
fn double_init_returns_already_initialized() {
    let (env, contract_id, factory, token_a, token_b, lp_token) = setup_env();
    let client = PairClient::new(&env, &contract_id);

    client.initialize(&factory, &token_a, &token_b, &lp_token);

    let result = client.try_initialize(&factory, &token_a, &token_b, &lp_token);
    assert_eq!(result, Err(Ok(PairError::AlreadyInitialized)));
}

#[test]
fn identical_tokens_returns_error() {
    let (env, contract_id, factory, token_a, _token_b, lp_token) = setup_env();
    let client = PairClient::new(&env, &contract_id);

    let result = client.try_initialize(&factory, &token_a, &token_a, &lp_token);
    assert_eq!(result, Err(Ok(PairError::InvalidInput)));
}

#[test]
fn get_reserves_returns_zeros_after_init() {
    let (env, contract_id, factory, token_a, token_b, lp_token) = setup_env();
    let client = PairClient::new(&env, &contract_id);

    client.initialize(&factory, &token_a, &token_b, &lp_token);

    let (r_a, r_b, ts) = client.get_reserves();
    assert_eq!(r_a, 0);
    assert_eq!(r_b, 0);
    assert_eq!(ts, 0); // default test env timestamp
}

#[test]
fn fee_state_has_sane_defaults() {
    let (env, contract_id, factory, token_a, token_b, lp_token) = setup_env();
    let client = PairClient::new(&env, &contract_id);

    client.initialize(&factory, &token_a, &token_b, &lp_token);

    // Default fee should be 30 bps (no volatility yet)
    let fee = client.get_current_fee_bps();
    assert_eq!(fee, 30);
}

#[test]
fn pair_state_stores_correct_addresses() {
    let (env, contract_id, factory, token_a, token_b, lp_token) = setup_env();
    let client = PairClient::new(&env, &contract_id);

    client.initialize(&factory, &token_a, &token_b, &lp_token);

    env.as_contract(&contract_id, || {
        let state = get_pair_state(&env).expect("PairStorage missing");
        assert_eq!(state.factory, factory);
        assert_eq!(state.token_a, token_a);
        assert_eq!(state.token_b, token_b);
        assert_eq!(state.lp_token, lp_token);
        assert_eq!(state.k_last, 0);
        assert_eq!(state.price_a_cumulative, 0);
        assert_eq!(state.price_b_cumulative, 0);
    });
}
