use crate::errors::RouterError;
use soroban_sdk::{contractclient, Address, Env};

#[contractclient(name = "FactoryClient")]
pub trait FactoryInterface {
    fn get_pair(env: Env, token_a: Address, token_b: Address) -> Option<Address>;
}

#[contractclient(name = "PairClient")]
pub trait PairInterface {
    fn burn(env: Env, to: Address) -> (i128, i128);
    fn lp_token(env: Env) -> Address;
}

#[contractclient(name = "TokenClient")]
pub trait TokenInterface {
    fn transfer(env: Env, from: Address, to: Address, amount: i128);
    fn balance(env: Env, id: Address) -> i128;
}

/// Computed output amount for an exact input swap using constant-product formula.
pub fn get_amount_out(
    _env: &Env,
    _amount_in: i128,
    _reserve_in: i128,
    _reserve_out: i128,
    _fee_bps: u32,
) -> Result<i128, RouterError> {
    todo!()
}

/// Computed input amount required for an exact output swap.
pub fn get_amount_in(
    _env: &Env,
    _amount_out: i128,
    _reserve_in: i128,
    _reserve_out: i128,
    _fee_bps: u32,
) -> Result<i128, RouterError> {
    todo!()
}

/// Sorted token addresses into canonical order.
pub fn sort_tokens(
    _token_a: &Address,
    _token_b: &Address,
) -> Result<(Address, Address), RouterError> {
    todo!()
}

/// Get the pair address from the factory contract
pub fn get_pair_address(
    env: &Env,
    factory: &Address,
    token_a: &Address,
    token_b: &Address,
) -> Result<Address, RouterError> {
    let factory_client = FactoryClient::new(env, factory);
    factory_client.get_pair(token_a, token_b).ok_or(RouterError::PairNotFound)
}
