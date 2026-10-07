use crate::errors::RouterError;
use coralswap_shared::{quote_input_amount, quote_output_amount, QuoteError};
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
    fn get_effective_fee_bps(env: Env) -> (u32, bool);
}

#[contractclient(name = "TokenClient")]
#[allow(dead_code)]
pub trait TokenInterface {
    fn transfer(env: Env, from: Address, to: Address, amount: i128);
    fn balance(env: Env, id: Address) -> i128;
}

/// Maps the shared quote error onto the router's error codes.
fn map_quote_error(err: QuoteError) -> RouterError {
    match err {
        QuoteError::ZeroAmount => RouterError::ZeroAmount,
        QuoteError::Dust => RouterError::DustAmount,
        QuoteError::InsufficientLiquidity | QuoteError::Overflow | QuoteError::InvalidFee => {
            RouterError::InsufficientLiquidity
        }
    }
}

/// Computes output amount for an exact input swap.
///
/// Thin wrapper over [`coralswap_shared::quote_output_amount`] — the single
/// quote implementation shared with the pair. `fee_bps` must already be the
/// pair's effective fee (see [`get_pair_reserves_and_fee`]).
#[allow(dead_code)]
pub fn get_amount_out(
    _env: &Env,
    amount_in: i128,
    reserve_in: i128,
    reserve_out: i128,
    fee_bps: u32,
) -> Result<i128, RouterError> {
    quote_output_amount(amount_in, reserve_in, reserve_out, fee_bps).map_err(map_quote_error)
}

/// Computes input amount required for an exact output swap.
///
/// Thin wrapper over [`coralswap_shared::quote_input_amount`]. `fee_bps` must
/// already be the pair's effective fee (see [`get_pair_reserves_and_fee`]).
#[allow(dead_code)]
pub fn get_amount_in(
    _env: &Env,
    amount_out: i128,
    reserve_in: i128,
    reserve_out: i128,
    fee_bps: u32,
) -> Result<i128, RouterError> {
    quote_input_amount(amount_out, reserve_in, reserve_out, fee_bps).map_err(map_quote_error)
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
///
/// Issue #350: Uses `get_effective_fee_bps()` instead of `get_current_fee_bps()`
/// to account for factory overrides, ensuring quotes match actual swap execution.
pub fn get_pair_reserves_and_fee(
    env: &Env,
    pair: &Address,
    token_in: &Address,
    token_out: &Address,
) -> Result<(i128, i128, u32), RouterError> {
    let pair_client = PairClient::new(env, pair);
    let (reserve_a, reserve_b, _) = pair_client.get_reserves();
    // NEW: Use effective fee (override-aware) instead of just current fee
    let fee_result = pair_client.try_get_effective_fee_bps();
    let (fee_bps, _is_override) = match fee_result {
        Ok(Ok(result)) => result,
        _ => return Err(RouterError::InternalError),
    };

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
        // A zero minimum (caller opted out of slippage protection) constrains
        // no earlier hop; get_amount_in rejects a zero output.
        current_min_output = if current_min_output <= 0 {
            0
        } else {
            get_amount_in(env, current_min_output, reserve_in, reserve_out, fee_bps)?
        };
    }
    Ok(min_amounts)
}
