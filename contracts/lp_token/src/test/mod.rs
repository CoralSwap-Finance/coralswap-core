use crate::errors::LpTokenError;
use crate::{LpToken, LpTokenClient};
use soroban_sdk::{testutils::Address as _, Address, Env, String};

fn setup_env() -> (Env, Address, LpTokenClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, LpToken);
    let client = LpTokenClient::new(&env, &contract_id);
    let admin = Address::generate(&env);

    (env, contract_id, client, admin)
}

fn initialize_client(client: &LpTokenClient<'_>, env: &Env, admin: &Address) {
    client.initialize(admin, &7, &String::from_str(env, "Coral LP"), &String::from_str(env, "CLP"));
}

#[test]
fn decimals_returns_not_initialized_when_metadata_missing() {
    let (_env, _contract_id, client, _admin) = setup_env();

    let result = client.try_decimals();
    assert_eq!(result, Err(Ok(LpTokenError::NotInitialized)));
}

#[test]
fn name_returns_not_initialized_when_metadata_missing() {
    let (_env, _contract_id, client, _admin) = setup_env();

    let result = client.try_name();
    assert_eq!(result, Err(Ok(LpTokenError::NotInitialized)));
}

#[test]
fn symbol_returns_not_initialized_when_metadata_missing() {
    let (_env, _contract_id, client, _admin) = setup_env();

    let result = client.try_symbol();
    assert_eq!(result, Err(Ok(LpTokenError::NotInitialized)));
}

#[test]
fn metadata_reads_return_stored_values_after_initialize() {
    let (env, _contract_id, client, admin) = setup_env();

    initialize_client(&client, &env, &admin);

    assert_eq!(client.decimals(), 7);
    assert_eq!(client.name(), String::from_str(&env, "Coral LP"));
    assert_eq!(client.symbol(), String::from_str(&env, "CLP"));
}

#[test]
fn burn_reduces_balance_and_total_supply() {
    let (env, _contract_id, client, admin) = setup_env();
    initialize_client(&client, &env, &admin);

    let user = Address::generate(&env);
    client.mint(&user, &1000);

    client.burn(&user, &400);

    assert_eq!(client.balance(&user), 600);
    assert_eq!(client.total_supply(), 600);
}

#[test]
fn burn_more_than_balance_returns_insufficient_balance() {
    let (env, _contract_id, client, admin) = setup_env();
    initialize_client(&client, &env, &admin);

    let user = Address::generate(&env);
    client.mint(&user, &500);

    let result = client.try_burn(&user, &501);
    assert_eq!(result, Err(Ok(LpTokenError::InsufficientBalance)));

    // Supply must remain intact
    assert_eq!(client.total_supply(), 500);
}

#[test]
fn burn_exact_balance_zeroes_supply() {
    let (env, _contract_id, client, admin) = setup_env();
    initialize_client(&client, &env, &admin);

    let user = Address::generate(&env);
    client.mint(&user, &100);

    client.burn(&user, &100);

    assert_eq!(client.balance(&user), 0);
    assert_eq!(client.total_supply(), 0);
}
