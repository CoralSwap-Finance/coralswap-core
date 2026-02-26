
use crate::{LpToken, LpTokenClient};
use soroban_sdk::{testutils::Address as _, Address, Env, String};

fn setup_test(env: &Env) -> (LpTokenClient<'_>, Address) {
    let admin = Address::generate(env);
    let lp_token_id = env.register_contract(None, LpToken);
    let lp_token = LpTokenClient::new(env, &lp_token_id);

    lp_token.initialize(
        &admin,
        &7,
        &String::from_str(env, "CoralSwap LP Token"),
        &String::from_str(env, "CLP"),
    );

    (lp_token, admin)
}

#[test]
fn test_mint_valid() {
    let env = Env::default();
    let (lp_token, _admin) = setup_test(&env);
    let user = Address::generate(&env);

    env.mock_all_auths();
    lp_token.mint(&user, &1000);

    assert_eq!(lp_token.balance(&user), 1000);
    assert_eq!(lp_token.total_supply(), 1000);
}

#[test]
fn test_mint_invalid_amount() {
    let env = Env::default();
    let (lp_token, _admin) = setup_test(&env);
    let user = Address::generate(&env);

    env.mock_all_auths();

    // Test zero amount
    let result = lp_token.try_mint(&user, &0);
    assert!(result.is_err());

    // Test negative amount
    let result = lp_token.try_mint(&user, &-1);
    assert!(result.is_err());
}

#[test]
fn test_mint_exceeds_max_supply() {
    let env = Env::default();
    let (lp_token, _admin) = setup_test(&env);
    let user = Address::generate(&env);

    env.mock_all_auths();

    // Mint maximum possible i128
    lp_token.mint(&user, &i128::MAX);
    assert_eq!(lp_token.total_supply(), i128::MAX);

    // Try to mint more
    let result = lp_token.try_mint(&user, &1);
    assert!(result.is_err());
}

#[test]
fn test_transfer_and_burn() {
    let env = Env::default();
    let (lp_token, _admin) = setup_test(&env);
    let user_1 = Address::generate(&env);
    let user_2 = Address::generate(&env);

    env.mock_all_auths();
    lp_token.mint(&user_1, &1000);

    lp_token.transfer(&user_1, &user_2, &400);
    assert_eq!(lp_token.balance(&user_1), 600);
    assert_eq!(lp_token.balance(&user_2), 400);

    lp_token.burn(&user_2, &100);
    assert_eq!(lp_token.balance(&user_2), 300);
    assert_eq!(lp_token.total_supply(), 900);
}
