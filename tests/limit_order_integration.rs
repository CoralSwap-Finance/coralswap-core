#![cfg(test)]

use soroban_sdk::{testutils::Address as _, token, Address, Env};

fn create_token<'a>(env: &Env, admin: &Address) -> (Address, token::Client<'a>, token::StellarAssetClient<'a>) {
    let sac = env.register_stellar_asset_contract_v2(admin.clone());
    let addr = sac.address();
    (
        addr.clone(),
        token::Client::new(env, &addr),
        token::StellarAssetClient::new(env, &addr),
    )
}

#[test]
fn test_limit_order_fill_lifecycle() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let keeper = Address::generate(&env);

    let (_token_a, token_a_client, token_a_admin) = create_token(&env, &admin);
    let (_token_b, token_b_client, token_b_admin) = create_token(&env, &admin);

    token_a_admin.mint(&user, &1_000_000);
    token_b_admin.mint(&keeper, &1_000_000);

    assert_eq!(token_a_client.balance(&user), 1_000_000);
    assert_eq!(token_b_client.balance(&keeper), 1_000_000);
    assert_eq!(token_b_client.balance(&user), 0);
    assert_eq!(token_a_client.balance(&keeper), 0);
}
