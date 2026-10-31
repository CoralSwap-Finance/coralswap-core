//! Governance token with on-chain vote delegation for CoralSwap.
//!
//! Implements a standard token interface (transfer, balance, approve, …)
//! plus vote delegation and historical voting-power checkpoints keyed by
//! ledger sequence.
//!
//! # Voting power
//!
//! Each token holder may delegate their voting power to any address,
//! including themselves (the default when no delegate has been set).  An
//! address's voting power is the sum of all token balances delegated to it.
//! Transfers, mints, and burns automatically adjust the voting power of the
//! affected delegates by writing new checkpoints.
//!
//! # Checkpoints
//!
//! Every time an address's voting power (or the total supply) changes, a
//! [`Checkpoint`] is appended to that address's checkpoint list.  Each
//! checkpoint records the ledger sequence at which the change took effect and
//! the new value.  Historical voting power can then be queried for any past
//! ledger via [`GovernanceToken::get_past_votes`], and historical total supply
//! via [`GovernanceToken::get_past_total_supply`], both using binary search
//! over the checkpoint list.
//!
//! Multiple changes within the same ledger collapse into a single checkpoint
//! (the last write wins), so querying at any past ledger always returns the
//! voting-power snapshot as of the end of that ledger.

#![no_std]

mod errors;
mod storage;

use errors::GovTokenError;
use soroban_sdk::{contract, contractimpl, Address, Env, String, Vec};
use storage::{AllowanceEntry, Checkpoint, GovTokenKey, TokenMetadata};

// Shared persistent-entry TTL policy.  See coralswap-shared for cadence math.
use coralswap_shared::{
    LP_PERSISTENT_EXTEND_TO as TTL_EXTEND_TO, LP_PERSISTENT_THRESHOLD as TTL_THRESHOLD,
};

#[contract]
pub struct GovernanceToken;

// `deprecated`: `Env::events().publish` is superseded by `#[contractevent]`;
// migration pending.
#[allow(deprecated)]
#[contractimpl]
impl GovernanceToken {
    // ── Initialization ──────────────────────────────────────────────────

    /// Initialize the governance token with metadata and admin.
    /// Can only be called once.
    pub fn initialize(
        env: Env,
        admin: Address,
        decimals: u32,
        name: String,
        symbol: String,
    ) -> Result<(), GovTokenError> {
        if env.storage().instance().has(&GovTokenKey::Admin) {
            return Err(GovTokenError::AlreadyInitialized);
        }

        if decimals > 18 {
            return Err(GovTokenError::InvalidMetadata);
        }
        if name.len() > 32 || symbol.len() > 32 {
            return Err(GovTokenError::InvalidMetadata);
        }

        env.storage().instance().set(&GovTokenKey::Admin, &admin);
        env.storage()
            .instance()
            .set(&GovTokenKey::Metadata, &TokenMetadata { decimals, name, symbol });
        env.storage().instance().set(&GovTokenKey::TotalSupply, &0i128);

        Ok(())
    }

    // ── Admin ───────────────────────────────────────────────────────────

    /// Returns the current admin.
    pub fn admin(env: Env) -> Result<Address, GovTokenError> {
        env.storage().instance().get(&GovTokenKey::Admin).ok_or(GovTokenError::NotInitialized)
    }

    /// Transfer admin role to a new address.  Only callable by the current admin.
    pub fn admin_transfer(env: Env, new_admin: Address) -> Result<(), GovTokenError> {
        let old_admin: Address = env
            .storage()
            .instance()
            .get(&GovTokenKey::Admin)
            .ok_or(GovTokenError::NotInitialized)?;

        old_admin.require_auth();
        env.storage().instance().set(&GovTokenKey::Admin, &new_admin);
        env.events().publish((soroban_sdk::symbol_short!("adm_xfer"), old_admin, new_admin), ());

        Ok(())
    }

    // ── Standard token interface ─────────────────────────────────────────

    /// Get the balance of an address.
    pub fn balance(env: Env, id: Address) -> i128 {
        let key = GovTokenKey::Balance(id);
        let bal: i128 = env.storage().persistent().get(&key).unwrap_or(0);
        if bal > 0 {
            env.storage().persistent().extend_ttl(&key, TTL_THRESHOLD, TTL_EXTEND_TO);
        }
        bal
    }

    /// Transfer tokens from `from` to `to`.
    /// Requires authorization from `from`.
    pub fn transfer(
        env: Env,
        from: Address,
        to: Address,
        amount: i128,
    ) -> Result<(), GovTokenError> {
        from.require_auth();
        Self::transfer_internal(&env, &from, &to, amount)?;
        Ok(())
    }

    /// Set allowance for `spender` to transfer from `from`.
    /// Requires authorization from `from`.
    pub fn approve(
        env: Env,
        from: Address,
        spender: Address,
        amount: i128,
        expiration_ledger: u32,
    ) -> Result<(), GovTokenError> {
        from.require_auth();

        if amount != 0 && expiration_ledger <= env.ledger().sequence() {
            return Err(GovTokenError::InvalidExpiration);
        }

        let key = GovTokenKey::Allowance(from.clone(), spender.clone());
        if amount == 0 {
            env.storage().persistent().remove(&key);
        } else {
            let entry = AllowanceEntry { amount, expiration_ledger };
            env.storage().persistent().set(&key, &entry);
            let ledgers_to_live = expiration_ledger.saturating_sub(env.ledger().sequence());
            env.storage().persistent().extend_ttl(&key, ledgers_to_live, ledgers_to_live);
        }

        env.events().publish(
            (soroban_sdk::symbol_short!("approve"), from, spender),
            (amount, expiration_ledger),
        );
        Ok(())
    }

    /// Get the allowance for `spender` to transfer from `from`.
    pub fn allowance(env: Env, from: Address, spender: Address) -> i128 {
        let key = GovTokenKey::Allowance(from, spender);
        if let Some(entry) = env.storage().persistent().get::<GovTokenKey, AllowanceEntry>(&key) {
            if entry.expiration_ledger <= env.ledger().sequence() {
                return 0;
            }
            entry.amount
        } else {
            0
        }
    }

    /// Transfer tokens from `from` to `to` using `spender`'s allowance.
    /// Requires authorization from `spender`.
    ///
    /// When `spender == from` (self-spend), allowance checking is bypassed,
    /// behaving like [`transfer`](Self::transfer).
    pub fn transfer_from(
        env: Env,
        spender: Address,
        from: Address,
        to: Address,
        amount: i128,
    ) -> Result<(), GovTokenError> {
        spender.require_auth();

        if spender != from {
            Self::spend_allowance(&env, &from, &spender, amount)?;
        }

        Self::transfer_internal(&env, &from, &to, amount)?;
        Ok(())
    }

    /// Mint new tokens to an address.  Only callable by admin.
    pub fn mint(env: Env, to: Address, amount: i128) -> Result<(), GovTokenError> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&GovTokenKey::Admin)
            .ok_or(GovTokenError::NotInitialized)?;
        admin.require_auth();

        if amount < 0 {
            return Err(GovTokenError::Overflow);
        }
        if amount == 0 {
            return Ok(());
        }

        const MAX_MINT_PER_CALL: i128 = 1_000_000_000_000_000_000;
        if amount > MAX_MINT_PER_CALL {
            return Err(GovTokenError::MintAmountTooLarge);
        }

        // Increase balance
        let balance_key = GovTokenKey::Balance(to.clone());
        let current: i128 = env.storage().persistent().get(&balance_key).unwrap_or(0);
        let new_balance = current.checked_add(amount).ok_or(GovTokenError::Overflow)?;
        Self::write_balance(&env, &balance_key, new_balance);

        // Increase total supply
        let total: i128 = env.storage().instance().get(&GovTokenKey::TotalSupply).unwrap_or(0);
        let new_total = total.checked_add(amount).ok_or(GovTokenError::Overflow)?;
        const MAX_TOTAL_SUPPLY: i128 = 1_000_000_000_000_000_000_000_000_000;
        if new_total > MAX_TOTAL_SUPPLY {
            return Err(GovTokenError::TotalSupplyExceeded);
        }
        env.storage().instance().set(&GovTokenKey::TotalSupply, &new_total);

        // Increase voting power of to's delegate
        let to_delegate = Self::get_delegate_or_self(&env, &to);
        Self::increase_votes(&env, &to_delegate, amount)?;

        // Write total-supply checkpoint
        Self::write_supply_checkpoint(&env, new_total);

        env.events().publish((soroban_sdk::symbol_short!("mint"), admin, to), amount);
        Ok(())
    }

    /// Burn tokens from an address.  Requires authorization from `from`.
    pub fn burn(env: Env, from: Address, amount: i128) -> Result<(), GovTokenError> {
        from.require_auth();
        Self::burn_internal(&env, &from, amount)?;
        Ok(())
    }

    /// Burn tokens via allowance.  Requires authorization from `spender`.
    pub fn burn_from(
        env: Env,
        spender: Address,
        from: Address,
        amount: i128,
    ) -> Result<(), GovTokenError> {
        spender.require_auth();
        Self::spend_allowance(&env, &from, &spender, amount)?;
        Self::burn_internal(&env, &from, amount)?;
        Ok(())
    }

    /// Get the number of decimals.
    pub fn decimals(env: Env) -> Result<u32, GovTokenError> {
        let metadata: TokenMetadata = env
            .storage()
            .instance()
            .get(&GovTokenKey::Metadata)
            .ok_or(GovTokenError::NotInitialized)?;
        Ok(metadata.decimals)
    }

    /// Get the token name.
    pub fn name(env: Env) -> Result<String, GovTokenError> {
        let metadata: TokenMetadata = env
            .storage()
            .instance()
            .get(&GovTokenKey::Metadata)
            .ok_or(GovTokenError::NotInitialized)?;
        Ok(metadata.name)
    }

    /// Get the token symbol.
    pub fn symbol(env: Env) -> Result<String, GovTokenError> {
        let metadata: TokenMetadata = env
            .storage()
            .instance()
            .get(&GovTokenKey::Metadata)
            .ok_or(GovTokenError::NotInitialized)?;
        Ok(metadata.symbol)
    }

    /// Get the total token supply.
    pub fn total_supply(env: Env) -> i128 {
        env.storage().instance().get(&GovTokenKey::TotalSupply).unwrap_or(0)
    }

    // ── Delegation & voting power ───────────────────────────────────────

    /// Delegate voting power to `to`.
    ///
    /// The caller (`from`) moves all of their token balance's voting power
    /// from their current delegate to `to`.  Self-delegation is allowed and
    /// is the default when no delegate has been set.
    pub fn delegate(env: Env, from: Address, to: Address) -> Result<(), GovTokenError> {
        from.require_auth();

        let current_delegate = Self::get_delegate_or_self(&env, &from);
        let bal: i128 =
            env.storage().persistent().get(&GovTokenKey::Balance(from.clone())).unwrap_or(0);

        // Record the new delegate
        let delegate_key = GovTokenKey::Delegate(from.clone());
        env.storage().persistent().set(&delegate_key, &to);
        env.storage().persistent().extend_ttl(&delegate_key, TTL_THRESHOLD, TTL_EXTEND_TO);

        // Move voting power from the old delegate to the new one
        Self::move_voting_power(&env, &current_delegate, &to, bal)?;

        env.events().publish((soroban_sdk::symbol_short!("delegate"), from, to), bal);
        Ok(())
    }

    /// Returns the address `addr` has delegated their voting power to.
    /// If no delegate has been set, returns `addr` itself.
    pub fn delegates(env: Env, addr: Address) -> Address {
        Self::get_delegate_or_self(&env, &addr)
    }

    /// Returns the current voting power of `addr` (including delegated votes).
    pub fn get_votes(env: Env, addr: Address) -> i128 {
        Self::get_votes_internal(&env, &addr)
    }

    /// Returns the voting power of `addr` as of the end of `ledger`.
    ///
    /// Returns [`GovTokenError::FutureLedger`] if `ledger` is greater than the
    /// current ledger sequence.
    pub fn get_past_votes(env: Env, addr: Address, ledger: u32) -> Result<i128, GovTokenError> {
        if ledger > env.ledger().sequence() {
            return Err(GovTokenError::FutureLedger);
        }
        let key = GovTokenKey::Checkpoints(addr);
        let checkpoints: Vec<Checkpoint> =
            env.storage().persistent().get(&key).unwrap_or(Vec::new(&env));
        Ok(Self::lookup_checkpoint(&checkpoints, ledger))
    }

    /// Returns the total token supply as of the end of `ledger`.
    ///
    /// Returns [`GovTokenError::FutureLedger`] if `ledger` is greater than the
    /// current ledger sequence.
    pub fn get_past_total_supply(env: Env, ledger: u32) -> Result<i128, GovTokenError> {
        if ledger > env.ledger().sequence() {
            return Err(GovTokenError::FutureLedger);
        }
        let key = GovTokenKey::TotalSupplyCheckpoints;
        let checkpoints: Vec<Checkpoint> =
            env.storage().persistent().get(&key).unwrap_or(Vec::new(&env));
        Ok(Self::lookup_checkpoint(&checkpoints, ledger))
    }

    // ── Internal helpers ────────────────────────────────────────────────

    fn transfer_internal(
        env: &Env,
        from: &Address,
        to: &Address,
        amount: i128,
    ) -> Result<(), GovTokenError> {
        if amount < 0 {
            return Err(GovTokenError::InsufficientBalance);
        }
        if amount == 0 {
            return Ok(());
        }

        // Debit sender
        let from_key = GovTokenKey::Balance(from.clone());
        let from_balance: i128 = env.storage().persistent().get(&from_key).unwrap_or(0);
        if from_balance < amount {
            return Err(GovTokenError::InsufficientBalance);
        }
        Self::write_balance(env, &from_key, from_balance - amount);

        // Credit receiver
        let to_key = GovTokenKey::Balance(to.clone());
        let to_balance: i128 = env.storage().persistent().get(&to_key).unwrap_or(0);
        let new_to_balance = to_balance.checked_add(amount).ok_or(GovTokenError::Overflow)?;
        Self::write_balance(env, &to_key, new_to_balance);

        // Move voting power from sender's delegate to receiver's delegate
        let from_delegate = Self::get_delegate_or_self(env, from);
        let to_delegate = Self::get_delegate_or_self(env, to);
        Self::move_voting_power(env, &from_delegate, &to_delegate, amount)?;

        env.events()
            .publish((soroban_sdk::symbol_short!("transfer"), from.clone(), to.clone()), amount);
        Ok(())
    }

    fn burn_internal(env: &Env, from: &Address, amount: i128) -> Result<(), GovTokenError> {
        if amount < 0 {
            return Err(GovTokenError::InsufficientBalance);
        }
        if amount == 0 {
            return Ok(());
        }

        let balance_key = GovTokenKey::Balance(from.clone());
        let current: i128 = env.storage().persistent().get(&balance_key).unwrap_or(0);
        if current < amount {
            return Err(GovTokenError::InsufficientBalance);
        }
        Self::write_balance(env, &balance_key, current - amount);

        // Decrease total supply
        let total: i128 = env.storage().instance().get(&GovTokenKey::TotalSupply).unwrap_or(0);
        let new_total = total.checked_sub(amount).ok_or(GovTokenError::InsufficientBalance)?;
        env.storage().instance().set(&GovTokenKey::TotalSupply, &new_total);

        // Decrease voting power of from's delegate
        let from_delegate = Self::get_delegate_or_self(env, from);
        Self::decrease_votes(env, &from_delegate, amount)?;

        Self::write_supply_checkpoint(env, new_total);

        env.events().publish((soroban_sdk::symbol_short!("burn"), from.clone()), amount);
        Ok(())
    }

    fn spend_allowance(
        env: &Env,
        from: &Address,
        spender: &Address,
        amount: i128,
    ) -> Result<(), GovTokenError> {
        let key = GovTokenKey::Allowance(from.clone(), spender.clone());
        let entry: AllowanceEntry =
            env.storage().persistent().get(&key).ok_or(GovTokenError::InsufficientAllowance)?;

        if entry.expiration_ledger <= env.ledger().sequence() {
            return Err(GovTokenError::InsufficientAllowance);
        }
        if entry.amount < amount {
            return Err(GovTokenError::InsufficientAllowance);
        }

        let new_amount = entry.amount - amount;
        if new_amount == 0 {
            env.storage().persistent().remove(&key);
        } else {
            let new_entry =
                AllowanceEntry { amount: new_amount, expiration_ledger: entry.expiration_ledger };
            env.storage().persistent().set(&key, &new_entry);
            let ledgers_to_live = entry.expiration_ledger.saturating_sub(env.ledger().sequence());
            env.storage().persistent().extend_ttl(&key, ledgers_to_live, ledgers_to_live);
        }
        Ok(())
    }

    fn write_balance(env: &Env, key: &GovTokenKey, balance: i128) {
        if balance == 0 {
            env.storage().persistent().remove(key);
        } else {
            env.storage().persistent().set(key, &balance);
            env.storage().persistent().extend_ttl(key, TTL_THRESHOLD, TTL_EXTEND_TO);
        }
    }

    fn get_delegate_or_self(env: &Env, addr: &Address) -> Address {
        env.storage().persistent().get(&GovTokenKey::Delegate(addr.clone())).unwrap_or(addr.clone())
    }

    fn get_votes_internal(env: &Env, addr: &Address) -> i128 {
        let key = GovTokenKey::Checkpoints(addr.clone());
        let checkpoints: Vec<Checkpoint> =
            env.storage().persistent().get(&key).unwrap_or(Vec::new(env));
        let len = checkpoints.len();
        if len == 0 {
            return 0;
        }
        checkpoints.get_unchecked(len - 1).value
    }

    /// Writes a checkpoint for `key`, collapsing same-ledger writes into the
    /// last entry so each ledger has at most one checkpoint.
    fn write_checkpoint(env: &Env, key: &GovTokenKey, new_value: i128) {
        let mut checkpoints: Vec<Checkpoint> =
            env.storage().persistent().get(key).unwrap_or(Vec::new(env));
        let current_ledger = env.ledger().sequence();

        let len = checkpoints.len();
        if len > 0 {
            let last = checkpoints.get_unchecked(len - 1);
            if last.ledger == current_ledger {
                checkpoints.pop_back();
            }
        }

        checkpoints.push_back(Checkpoint { ledger: current_ledger, value: new_value });

        env.storage().persistent().set(key, &checkpoints);
        env.storage().persistent().extend_ttl(key, TTL_THRESHOLD, TTL_EXTEND_TO);
    }

    fn write_supply_checkpoint(env: &Env, new_supply: i128) {
        Self::write_checkpoint(env, &GovTokenKey::TotalSupplyCheckpoints, new_supply);
    }

    fn increase_votes(env: &Env, addr: &Address, amount: i128) -> Result<(), GovTokenError> {
        if amount == 0 {
            return Ok(());
        }
        let votes = Self::get_votes_internal(env, addr);
        let new_votes = votes.checked_add(amount).ok_or(GovTokenError::Overflow)?;
        Self::write_checkpoint(env, &GovTokenKey::Checkpoints(addr.clone()), new_votes);
        Ok(())
    }

    fn decrease_votes(env: &Env, addr: &Address, amount: i128) -> Result<(), GovTokenError> {
        if amount == 0 {
            return Ok(());
        }
        let votes = Self::get_votes_internal(env, addr);
        let new_votes = votes.checked_sub(amount).ok_or(GovTokenError::Overflow)?;
        Self::write_checkpoint(env, &GovTokenKey::Checkpoints(addr.clone()), new_votes);
        Ok(())
    }

    fn move_voting_power(
        env: &Env,
        from: &Address,
        to: &Address,
        amount: i128,
    ) -> Result<(), GovTokenError> {
        if amount == 0 || from == to {
            return Ok(());
        }
        Self::decrease_votes(env, from, amount)?;
        Self::increase_votes(env, to, amount)?;
        Ok(())
    }

    /// Binary-searches `checkpoints` for the largest checkpoint whose `ledger`
    /// is `<= target`.  Returns 0 if no such checkpoint exists.
    fn lookup_checkpoint(checkpoints: &Vec<Checkpoint>, target: u32) -> i128 {
        if checkpoints.is_empty() {
            return 0;
        }
        let mut lo: u32 = 0;
        let mut hi: u32 = checkpoints.len();
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            let cp = checkpoints.get_unchecked(mid);
            if cp.ledger <= target {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        if lo == 0 {
            return 0;
        }
        checkpoints.get_unchecked(lo - 1).value
    }
}

#[cfg(test)]
mod test;

// ─────────────────────────────────────────────────────────────────────────
// Compile-time naming-convention checks (issue #382).
//
// Every emitted topic symbol must be lowercase snake_case and ≤ 9 chars for
// `symbol_short!` topics. The list below mirrors every literal emitted by
// this contract; update it when a symbol is added or renamed so the build
// fails if the convention is broken.
// ─────────────────────────────────────────────────────────────────────────

const fn is_convention_symbol(name: &str) -> bool {
    let bytes = name.as_bytes();
    if bytes.is_empty() {
        return false;
    }
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if !(b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_') {
            return false;
        }
        i += 1;
    }
    true
}

const _: () = assert!(is_convention_symbol("adm_xfer"));
const _: () = assert!(is_convention_symbol("approve"));
const _: () = assert!(is_convention_symbol("mint"));
const _: () = assert!(is_convention_symbol("burn"));
const _: () = assert!(is_convention_symbol("transfer"));
const _: () = assert!(is_convention_symbol("delegate"));
