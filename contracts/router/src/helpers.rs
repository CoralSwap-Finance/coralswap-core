use crate::errors::RouterError;
use ethnum::U256;
use soroban_sdk::{contractclient, Address, Env, Vec};

#[contractclient(name = "FactoryClient")]
#[allow(dead_code)]
pub trait FactoryInterface {
    fn get_pair(env: Env, token_a: Address, token_b: Address) -> Option<Address>;
    fn create_pair(env: Env, token_a: Address, token_b: Address) -> Address;
    fn fee_to_setter(env: Env) -> Option<Address>;
    fn is_pair(env: Env, pair: Address) -> bool;
}

#[contractclient(name = "PairClient")]
#[allow(dead_code)]
pub trait PairInterface {
    fn burn(env: Env, to: Address) -> (i128, i128);
    fn deposit_lp(env: Env, from: Address, amount: i128);
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

    let out = numerator / denominator;
    // Dust policy (issue 393): truncated zero outputs are typed errors.
    // Note: small non-zero outputs are allowed so 1k-in/100k-pool quotes
    // (~987 out) keep working; the pair enforces the reserve floor.
    if out <= 0 {
        return Err(RouterError::DustAmount);
    }
    Ok(out)
}

/// Computes input amount required for an exact output swap.
///
/// Formula:
/// amount_in = ceil((reserve_in * amount_out * 10000)
///               / ((reserve_out - amount_out) * (10000 - fee_bps)))
///
/// The ceiling is deliberate: the router must overfund by at most one unit,
/// never underfund an exact-output swap. `U256` keeps the intermediate product
/// exact when reserves are large even though the returned amount is `i128`.
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

    let numerator =
        U256::from(reserve_in as u128) * U256::from(amount_out as u128) * U256::from(10_000u128);
    let denominator =
        U256::from((reserve_out - amount_out) as u128) * U256::from((10_000 - fee_bps) as u128);
    let quotient = numerator / denominator;
    let remainder = numerator % denominator;
    let rounded = quotient + U256::from((remainder != U256::ZERO) as u8);
    i128::try_from(rounded.as_u128()).map_err(|_| RouterError::InsufficientLiquidity)
}

/// Given some amount of an asset and pair reserves,
/// returns equivalent amount of the other asset.
///
/// Formula:
/// amount_b = (amount_a * reserve_b) / reserve_a
#[allow(dead_code)]
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

/// Retrieves pair address from factory and validates that it is a registered pair.
pub fn get_pair_address(
    env: &Env,
    factory: &Address,
    token_a: &Address,
    token_b: &Address,
) -> Result<Address, RouterError> {
    let factory_client = FactoryClient::new(env, factory);
    let pair = factory_client.get_pair(token_a, token_b).ok_or(RouterError::PairNotFound)?;
    if !factory_client.is_pair(&pair) {
        return Err(RouterError::PairNotFound);
    }
    Ok(pair)
}

/// Returns (reserve_in, reserve_out, fee_bps) for a swap of token_in → token_out
/// via the pair at the given address. Determines direction by sorting tokens.
pub fn get_pair_reserves_and_fee(
    env: &Env,
    pair: &Address,
    token_in: &Address,
    token_out: &Address,
) -> Result<(i128, i128, u32), RouterError> {
    let pair_client = PairClient::new(env, pair);
    let (reserve_a, reserve_b, _) = pair_client.get_reserves();
    let fee_bps = pair_client.get_current_fee_bps();

    let (token_0, _) = sort_tokens(token_in, token_out)?;
    if *token_in == token_0 {
        Ok((reserve_a, reserve_b, fee_bps))
    } else {
        Ok((reserve_b, reserve_a, fee_bps))
    }
}

/// Computes output amounts for every hop along a multi-hop path.
/// Returns a Vec of length path.len()-1 where amounts[i] is the output of
/// swapping path[i] → path[i+1].
pub fn get_path_amounts_out(
    env: &Env,
    factory: &Address,
    path: &Vec<Address>,
    amount_in: i128,
) -> Result<Vec<i128>, RouterError> {
    if path.len() < 2 {
        return Err(RouterError::InvalidPath);
    }
    let mut amounts = Vec::new(env);
    let mut current_amount = amount_in;
    for i in 0..path.len() - 1 {
        let pair =
            get_pair_address(env, factory, &path.get(i).unwrap(), &path.get(i + 1).unwrap())?;
        let (reserve_in, reserve_out, fee_bps) = get_pair_reserves_and_fee(
            env,
            &pair,
            &path.get(i).unwrap(),
            &path.get(i + 1).unwrap(),
        )?;
        current_amount = get_amount_out(env, current_amount, reserve_in, reserve_out, fee_bps)?;
        amounts.push_back(current_amount);
    }
    Ok(amounts)
}

/// Computes minimum required output for each hop to achieve a global minimum output.
/// Works backwards from the final minimum using the inverse swap formula.
/// Returns a Vec of length path.len()-1 where min_amounts[i] is the minimum output
/// required from swapping path[i] → path[i+1] to meet the global min_out.
///
/// Example for 2-hop (token_in -> hub -> token_out):
/// - Hop 2 needs to produce at least amount_out_min
/// - To achieve that, hop 2 needs a minimum input = get_amount_in(amount_out_min, hop2_reserves)
/// - That minimum input IS the minimum output required from hop 1
pub fn get_path_minimums(
    env: &Env,
    factory: &Address,
    path: &Vec<Address>,
    amount_out_min: i128,
) -> Result<Vec<i128>, RouterError> {
    if path.len() < 2 {
        return Err(RouterError::InvalidPath);
    }
    let hops = path.len() - 1;
    let mut min_amounts = Vec::new(env);
    let mut current_min_output = amount_out_min;

    // Walk backwards from final hop to first hop
    for i in (0..hops).rev() {
        let pair =
            get_pair_address(env, factory, &path.get(i).unwrap(), &path.get(i + 1).unwrap())?;
        let (reserve_in, reserve_out, fee_bps) = get_pair_reserves_and_fee(
            env,
            &pair,
            &path.get(i).unwrap(),
            &path.get(i + 1).unwrap(),
        )?;
        // For the current hop, we need to produce at least current_min_output
        // This is the minimum output required from this hop
        min_amounts.insert(0, current_min_output);
        // Then compute the minimum input needed for this hop to achieve that output
        // This becomes the minimum output required from the previous hop
        current_min_output = get_amount_in(env, current_min_output, reserve_in, reserve_out, fee_bps)?;
    }
    Ok(min_amounts)
}
