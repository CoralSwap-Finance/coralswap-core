#![allow(dead_code)]

use crate::errors::RouterError;
use soroban_sdk::{contractclient, Address, Env};

#[contractclient(name = "FactoryClient")]
#[allow(dead_code)]
pub trait FactoryInterface {
    fn get_pair(env: Env, token_a: Address, token_b: Address) -> Option<Address>;
    fn create_pair(env: Env, token_a: Address, token_b: Address) -> Address;
}

#[contractclient(name = "PairClient")]
#[allow(dead_code)]
pub trait PairInterface {
    fn burn(env: Env, to: Address) -> (i128, i128);
    fn mint(env: Env, to: Address) -> i128;
    fn lp_token(env: Env) -> Address;
    fn swap(env: Env, amount_a_out: i128, amount_b_out: i128, to: Address);
    fn get_reserves(env: Env) -> (i128, i128, u64);
    fn get_current_fee_bps(env: Env) -> u32;
}

#[contractclient(name = "TokenClient")]
#[allow(dead_code)]
pub trait TokenInterface {
    fn transfer(env: Env, from: Address, to: Address, amount: i128);
    fn balance(env: Env, id: Address) -> i128;
}

/// Computes output amount for an exact input swap using constant-product formula.
///
/// Formula:
/// amount_out = (amount_in * (10000 - fee_bps) * reserve_out)
///              / (reserve_in * 10000 + amount_in * (10000 - fee_bps))
#[allow(dead_code)]
pub fn get_amount_out(
    _env: &Env,
    amount_in: i128,
    reserve_in: i128,
    reserve_out: i128,
    fee_bps: u32,
) -> Result<i128, RouterError> {
    if amount_in <= 0 {
        return Err(RouterError::ZeroAmount);
    }
    if reserve_in <= 0 || reserve_out <= 0 {
        return Err(RouterError::InsufficientLiquidity);
    }

    let amount_in_with_fee =
        amount_in.checked_mul(10000 - fee_bps as i128).ok_or(RouterError::InsufficientLiquidity)?;

    let numerator =
        amount_in_with_fee.checked_mul(reserve_out).ok_or(RouterError::InsufficientLiquidity)?;

    let denominator = reserve_in
        .checked_mul(10000)
        .ok_or(RouterError::InsufficientLiquidity)?
        .checked_add(amount_in_with_fee)
        .ok_or(RouterError::InsufficientLiquidity)?;

    Ok(numerator / denominator)
}

/// Computes input amount required for an exact output swap.
///
/// Formula:
/// amount_in = (reserve_in * amount_out * 10000)
///             / ((reserve_out - amount_out) * (10000 - fee_bps)) + 1
#[allow(dead_code)]
pub fn get_amount_in(
    _env: &Env,
    amount_out: i128,
    reserve_in: i128,
    reserve_out: i128,
    fee_bps: u32,
) -> Result<i128, RouterError> {
    if amount_out <= 0 {
        return Err(RouterError::ZeroAmount);
    }

    if reserve_in <= 0 || reserve_out <= 0 || amount_out >= reserve_out {
        return Err(RouterError::InsufficientLiquidity);
    }

    let numerator = reserve_in
        .checked_mul(amount_out)
        .ok_or(RouterError::InsufficientLiquidity)?
        .checked_mul(10000)
        .ok_or(RouterError::InsufficientLiquidity)?;

    let denominator = (reserve_out - amount_out)
        .checked_mul(10000 - fee_bps as i128)
        .ok_or(RouterError::InsufficientLiquidity)?;

    Ok((numerator / denominator) + 1)
}

/// Given some amount of an asset and pair reserves,
/// returns equivalent amount of the other asset.
///
/// Formula:
/// amount_b = (amount_a * reserve_b) / reserve_a
pub fn quote(amount_a: i128, reserve_a: i128, reserve_b: i128) -> Result<i128, RouterError> {
    if amount_a <= 0 {
        return Err(RouterError::ZeroAmount);
    }

    if reserve_a <= 0 || reserve_b <= 0 {
        return Err(RouterError::InsufficientLiquidity);
    }

    let amount_b =
        amount_a.checked_mul(reserve_b).ok_or(RouterError::InsufficientLiquidity)? / reserve_a;

    Ok(amount_b)
}

/// Sorts token addresses into canonical lexicographic order.
#[allow(dead_code)]
pub fn sort_tokens(
    token_a: &Address,
    token_b: &Address,
) -> Result<(Address, Address), RouterError> {
    if token_a == token_b {
        return Err(RouterError::IdenticalTokens);
    }

    let (token_0, token_1) = if token_a < token_b {
        (token_a.clone(), token_b.clone())
    } else {
        (token_b.clone(), token_a.clone())
    };

    Ok((token_0, token_1))
}

/// Computes optimal deposit amounts for liquidity provision.
pub fn compute_optimal_amounts(
    amount_a_desired: i128,
    amount_b_desired: i128,
    amount_a_min: i128,
    amount_b_min: i128,
    reserve_a: i128,
    reserve_b: i128,
) -> Result<(i128, i128), RouterError> {
    if reserve_a == 0 && reserve_b == 0 {
        return Ok((amount_a_desired, amount_b_desired));
    }

    let amount_b_optimal =
        amount_a_desired.checked_mul(reserve_b).ok_or(RouterError::InsufficientLiquidity)?
            / reserve_a;

    if amount_b_optimal <= amount_b_desired {
        if amount_b_optimal < amount_b_min {
            return Err(RouterError::SlippageExceeded);
        }
        Ok((amount_a_desired, amount_b_optimal))
    } else {
        let amount_a_optimal =
            amount_b_desired.checked_mul(reserve_a).ok_or(RouterError::InsufficientLiquidity)?
                / reserve_b;

        if amount_a_optimal < amount_a_min {
            return Err(RouterError::SlippageExceeded);
        }

        Ok((amount_a_optimal, amount_b_desired))
    }
}

/// Retrieves pair address from factory.
pub fn get_pair_address(
    env: &Env,
    factory: &Address,
    token_a: &Address,
    token_b: &Address,
) -> Result<Address, RouterError> {
    let factory_client = FactoryClient::new(env, factory);
    factory_client.get_pair(token_a, token_b).ok_or(RouterError::PairNotFound)
}
