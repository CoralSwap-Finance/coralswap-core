use soroban_sdk::{Address, Env};
use crate::errors::RouterError;

/// Computes output amount for an exact input swap using constant-product formula.
///
/// Formula: amount_out = (amount_in * (10000 - fee_bps) * reserve_out) / 
///                       (reserve_in * 10000 + amount_in * (10000 - fee_bps))
///
/// # Arguments
/// * `amount_in` - The input token amount
/// * `reserve_in` - The reserve of the input token in the pair
/// * `reserve_out` - The reserve of the output token in the pair
/// * `fee_bps` - The fee in basis points (e.g., 30 = 0.3%)
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
        return Err(RouterError::InsufficientOutputAmount);
    }

    // Calculate: amount_in * (10000 - fee_bps)
    let amount_in_with_fee = amount_in
        .checked_mul(10000 - fee_bps as i128)
        .ok_or(RouterError::ExcessiveInputAmount)?;

    // Calculate: amount_in_with_fee * reserve_out
    let numerator = amount_in_with_fee
        .checked_mul(reserve_out)
        .ok_or(RouterError::ExcessiveInputAmount)?;

    // Calculate: reserve_in * 10000 + amount_in_with_fee
    let denominator = reserve_in
        .checked_mul(10000)
        .ok_or(RouterError::ExcessiveInputAmount)?
        .checked_add(amount_in_with_fee)
        .ok_or(RouterError::ExcessiveInputAmount)?;

    // Final division
    let amount_out = numerator / denominator;

    if amount_out <= 0 {
        return Err(RouterError::InsufficientOutputAmount);
    }

    Ok(amount_out)
}

/// Computes input amount required for an exact output swap.
///
/// Formula: amount_in = (reserve_in * amount_out * 10000) / 
///                      ((reserve_out - amount_out) * (10000 - fee_bps)) + 1
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
    if reserve_in <= 0 || reserve_out <= 0 {
        return Err(RouterError::InsufficientOutputAmount);
    }
    if amount_out >= reserve_out {
        return Err(RouterError::InsufficientOutputAmount);
    }

    // Calculate: reserve_in * amount_out * 10000
    let numerator = reserve_in
        .checked_mul(amount_out)
        .ok_or(RouterError::ExcessiveInputAmount)?
        .checked_mul(10000)
        .ok_or(RouterError::ExcessiveInputAmount)?;

    // Calculate: (reserve_out - amount_out) * (10000 - fee_bps)
    let denominator = (reserve_out - amount_out)
        .checked_mul(10000 - fee_bps as i128)
        .ok_or(RouterError::ExcessiveInputAmount)?;

    // Final division with +1 to round up
    let amount_in = (numerator / denominator)
        .checked_add(1)
        .ok_or(RouterError::ExcessiveInputAmount)?;

    Ok(amount_in)
}

/// Sorts token addresses into canonical order (lexicographically).
///
/// Returns tokens in the order (token_a, token_b) where token_a < token_b.
/// This matches the ordering used by the Factory when creating pairs.
pub fn sort_tokens(
    token_a: &Address,
    token_b: &Address,
) -> Result<(Address, Address), RouterError> {
    if token_a == token_b {
        return Err(RouterError::IdenticalTokens);
    }

    if token_a < token_b {
        Ok((token_a.clone(), token_b.clone()))
    } else {
        Ok((token_b.clone(), token_a.clone()))
    }
}
