//! SEP-41 Liquidity Pool Token implementation for CoralSwap.
//!
//! This contract provides a standard-compliant LP token that can be
//! minted/burned by the authorized CoralSwap Pair contract.

#![no_std]

mod errors;
mod storage;

use errors::LpTokenError;
use soroban_sdk::{contract, contractimpl, Address, Env, String};
use storage::{AllowanceEntry, LpTokenKey, TokenMetadata};

#[contract]
pub struct LpToken;

#[contractimpl]
impl LpToken {
    /// Initialize the LP token with metadata and admin
    /// Can only be called once
    pub fn initialize(
        env: Env,
        admin: Address,
        decimals: u32,
        name: String,
        symbol: String,
    ) -> Result<(), LpTokenError> {
        // Check if already initialized
        if env.storage().instance().has(&LpTokenKey::Admin) {
            return Err(LpTokenError::AlreadyInitialized);
        }

        // Store admin
        env.storage().instance().set(&LpTokenKey::Admin, &admin);

        // Store metadata
        let metadata = TokenMetadata { decimals, name, symbol };
        env.storage().instance().set(&LpTokenKey::Metadata, &metadata);

        // Initialize total supply to 0
        env.storage().instance().set(&LpTokenKey::TotalSupply, &0i128);

        Ok(())
    }

    /// Get the allowance for spender to transfer from `from`
    pub fn allowance(env: Env, from: Address, spender: Address) -> i128 {
        let key = LpTokenKey::Allowance(from, spender);

        if let Some(allowance_entry) =
            env.storage().persistent().get::<LpTokenKey, AllowanceEntry>(&key)
        {
            // Check if allowance has expired
            if allowance_entry.expiration_ledger < env.ledger().sequence() {
                return 0;
            }
            allowance_entry.amount
        } else {
            0
        }
    }

    /// Set allowance for spender to transfer from `from`
    /// Requires authorization from `from`
    pub fn approve(
        env: Env,
        from: Address,
        spender: Address,
        amount: i128,
        expiration_ledger: u32,
    ) -> Result<(), LpTokenError> {
        // Require authorization from the `from` address
        from.require_auth();

        // Validate expiration ledger (unless setting to 0)
        if amount != 0 && expiration_ledger < env.ledger().sequence() {
            return Err(LpTokenError::Unauthorized);
        }

        let key = LpTokenKey::Allowance(from.clone(), spender.clone());

        if amount == 0 {
            // Remove allowance if amount is 0
            env.storage().persistent().remove(&key);
        } else {
            let allowance_entry = AllowanceEntry { amount, expiration_ledger };
            env.storage().persistent().set(&key, &allowance_entry);

            // Set TTL for the allowance entry
            let ledgers_to_live = expiration_ledger.saturating_sub(env.ledger().sequence());
            env.storage().persistent().extend_ttl(&key, ledgers_to_live, ledgers_to_live);
        }

        // Emit approve event
        env.events().publish(
            (soroban_sdk::symbol_short!("approve"), from, spender),
            (amount, expiration_ledger),
        );

        Ok(())
    }

    /// Get the balance of an address
    pub fn balance(env: Env, id: Address) -> i128 {
        let key = LpTokenKey::Balance(id);
        env.storage().persistent().get(&key).unwrap_or(0)
    }

    /// Transfer tokens from `from` to `to`
    /// Requires authorization from `from`
    pub fn transfer(
        env: Env,
        from: Address,
        to: Address,
        amount: i128,
    ) -> Result<(), LpTokenError> {
        // Require authorization from the `from` address
        from.require_auth();

        // Perform the transfer
        Self::transfer_internal(&env, &from, &to, amount)?;

        Ok(())
    }

    /// Transfer tokens from `from` to `to` using spender's allowance
    /// Requires authorization from `spender`
    pub fn transfer_from(
        env: Env,
        spender: Address,
        from: Address,
        to: Address,
        amount: i128,
    ) -> Result<(), LpTokenError> {
        // Require authorization from the spender
        spender.require_auth();

        // Check and deduct allowance
        Self::spend_allowance(&env, &from, &spender, amount)?;

        // Perform the transfer
        Self::transfer_internal(&env, &from, &to, amount)?;

        Ok(())
    }

    /// Mint new tokens to an address
    /// Only callable by admin (pair contract)
    pub fn mint(env: Env, to: Address, amount: i128) -> Result<(), LpTokenError> {
        // Get admin and require authorization
        let admin: Address =
            env.storage().instance().get(&LpTokenKey::Admin).ok_or(LpTokenError::NotInitialized)?;

        admin.require_auth();

        // Increase balance
        let balance_key = LpTokenKey::Balance(to.clone());
        let current_balance: i128 = env.storage().persistent().get(&balance_key).unwrap_or(0);
        let new_balance = current_balance.checked_add(amount).ok_or(LpTokenError::Overflow)?;
        Self::write_balance(env.storage(), &balance_key, new_balance);

        // Increase total supply
        let total_supply: i128 =
            env.storage().instance().get(&LpTokenKey::TotalSupply).unwrap_or(0);
        let new_total_supply = total_supply.checked_add(amount).ok_or(LpTokenError::Overflow)?;
        env.storage().instance().set(&LpTokenKey::TotalSupply, &new_total_supply);

        // Emit mint event
        env.events().publish((soroban_sdk::symbol_short!("mint"), admin, to), amount);

        Ok(())
    }

    /// Burn tokens from an address
    /// Requires authorization from `from`
    pub fn burn(env: Env, from: Address, amount: i128) -> Result<(), LpTokenError> {
        // Require authorization from the `from` address
        from.require_auth();

        // Decrease balance
        let balance_key = LpTokenKey::Balance(from.clone());
        let current_balance: i128 = env.storage().persistent().get(&balance_key).unwrap_or(0);

        if current_balance < amount {
            return Err(LpTokenError::InsufficientBalance);
        }

        let new_balance = current_balance - amount;
        Self::write_balance(env.storage(), &balance_key, new_balance);

        // Decrease total supply
        let total_supply: i128 =
            env.storage().instance().get(&LpTokenKey::TotalSupply).unwrap_or(0);
        let new_total_supply =
            total_supply.checked_sub(amount).ok_or(LpTokenError::InsufficientBalance)?;
        env.storage().instance().set(&LpTokenKey::TotalSupply, &new_total_supply);

        // Emit burn event
        env.events().publish((soroban_sdk::symbol_short!("burn"), from), amount);

        Ok(())
    }

    /// Get the number of decimals
    pub fn decimals(env: Env) -> Result<u32, LpTokenError> {
        let metadata: TokenMetadata = env
            .storage()
            .instance()
            .get(&LpTokenKey::Metadata)
            .ok_or(LpTokenError::NotInitialized)?;
        Ok(metadata.decimals)
    }

    /// Get the token name
    pub fn name(env: Env) -> Result<String, LpTokenError> {
        let metadata: TokenMetadata = env
            .storage()
            .instance()
            .get(&LpTokenKey::Metadata)
            .ok_or(LpTokenError::NotInitialized)?;
        Ok(metadata.name)
    }

    /// Get the token symbol
    pub fn symbol(env: Env) -> Result<String, LpTokenError> {
        let metadata: TokenMetadata = env
            .storage()
            .instance()
            .get(&LpTokenKey::Metadata)
            .ok_or(LpTokenError::NotInitialized)?;
        Ok(metadata.symbol)
    }

    /// Get the total supply
    pub fn total_supply(env: Env) -> i128 {
        env.storage().instance().get(&LpTokenKey::TotalSupply).unwrap_or(0)
    }

    // Internal helper functions

    /// Internal transfer function
    fn transfer_internal(
        env: &Env,
        from: &Address,
        to: &Address,
        amount: i128,
    ) -> Result<(), LpTokenError> {
        if amount < 0 {
            return Err(LpTokenError::InsufficientBalance);
        }

        if amount == 0 {
            return Ok(());
        }

        // Debit from sender
        let from_key = LpTokenKey::Balance(from.clone());
        let from_balance: i128 = env.storage().persistent().get(&from_key).unwrap_or(0);

        if from_balance < amount {
            return Err(LpTokenError::InsufficientBalance);
        }

        let new_from_balance = from_balance - amount;
        Self::write_balance(env.storage(), &from_key, new_from_balance);

        // Credit to receiver
        let to_key = LpTokenKey::Balance(to.clone());
        let to_balance: i128 = env.storage().persistent().get(&to_key).unwrap_or(0);
        let new_to_balance = to_balance.checked_add(amount).ok_or(LpTokenError::Overflow)?;
        Self::write_balance(env.storage(), &to_key, new_to_balance);

        // Emit transfer event
        env.events()
            .publish((soroban_sdk::symbol_short!("transfer"), from.clone(), to.clone()), amount);

        Ok(())
    }

    /// Internal function to spend allowance
    fn spend_allowance(
        env: &Env,
        from: &Address,
        spender: &Address,
        amount: i128,
    ) -> Result<(), LpTokenError> {
        let key = LpTokenKey::Allowance(from.clone(), spender.clone());

        let allowance_entry: AllowanceEntry =
            env.storage().persistent().get(&key).ok_or(LpTokenError::InsufficientAllowance)?;

        // Check if allowance has expired
        if allowance_entry.expiration_ledger < env.ledger().sequence() {
            return Err(LpTokenError::InsufficientAllowance);
        }

        // Check if allowance is sufficient
        if allowance_entry.amount < amount {
            return Err(LpTokenError::InsufficientAllowance);
        }

        // Deduct allowance
        let new_amount = allowance_entry.amount - amount;
        if new_amount == 0 {
            env.storage().persistent().remove(&key);
        } else {
            let new_allowance_entry = AllowanceEntry {
                amount: new_amount,
                expiration_ledger: allowance_entry.expiration_ledger,
            };
            env.storage().persistent().set(&key, &new_allowance_entry);
        }

        Ok(())
    }

    fn write_balance(storage: soroban_sdk::storage::Storage, key: &LpTokenKey, balance: i128) {
        if balance == 0 {
            storage.persistent().remove(key);
        } else {
            storage.persistent().set(key, &balance);
        }
    }
}

#[cfg(test)]
mod test;
