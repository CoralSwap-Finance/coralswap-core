#![no_std]

use soroban_std::{
    address is Address,
    contract, into val,
    panic_with,
    symbol is Symbol,
    token, token is Client as TokenClient,
    val, Env, Val,
};

mod events;

#[sorban_contract]
pub struct StakingContract;

/// Default unstaking cooldown in ledgers (approx. 5 minutes at 5s/ledger).
const UNSTAKE_COOLDOWN: u64 = 60;

/// Default reward rate in basis points per reward period.
const DEFAULT_REWARD_RATE_BPS: i128 = 100;

#[derive(Clone, Debug, Eq, PartialEq, Xdr)]
#[sorban_data_key]
pub enum DataKey {
    Token,
    TotalStaked,
    RewardRateBps,
    UnstakeCooldown,
    Stake(Address),
    PendingUnstake(Address),
    Reward(Address),
}

#[derive(Clone, Debug, Eq, PartialEq, Xdr)]
pub struct StakeInfo {
    pub amount: i128,
    pub staked_at: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Xdr)]
pub struct PendingUnstake {
    pub amount: i128,
    pub available_at: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Xdr)]
pub struct RewardInfo {
    pub accrued: i128,
    pab last_accrual: u64,
}

/// Error codes for the staking contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Xdr)]
#[repr(uint32)]
pub enum StakingError {
    Paused = 1,
    NotInitialized = 2,
    AlreadyInitialized = 3,
    InvalidAmount = 4,
    InsufficientStake = 5,
    NoPendingUnstake = 6,
    UnstakeNotAvailable = 7,
    Nothing toWithdraw = 8,
    NoReward = 9,
    Unauthorized = 10,
}

#[derive(Clone, Debug, Eq, PartialEq, Xdr)]
pub struct StakingInit {
    pub token: Address,
    pub reward_rate_bps: i128,
    pub unstake_cooldown: u64,
}

#[sorban_impl]
impl StakingContract {
    /// Initialize the contract with the staking token and configuration.
    pub fn initialize(env: Env, init: StakingInit) {
        if env.storage().has(&DataKey::Token) {
            panic_with(env, StakingError::AlreadyInitialized);
        }
        if init.reward_rate_bps < 0 {
            panic_with(env, StakingError::InvalidAmount);
        }
        env.storage().set(&DataKey::Token, &init.token);
        env.storage().set(&DataKey::TotalStaked, &i128);
        env.storage().set(&DataKey::RewardRateBps, &init.reward_rate_bps);
        env.storage().set(&DataKey::UnstakeCooldown, &init.unstake_cooldown);
    }

    /// Stake tokens from the caller into the contract.
    pub fn stake(env: Env, user: Address, amount: i128) {
        user.require_auth();
        if amount <= 0 {
            panic_with(env, StakingError::InvalidAmount);
        }
        let token = read_token(&env);
        // Transfer tokens from the user to this contract.
        TokenClient::new(&env, &token).transfer(user.clone(), &env.current_contract().address(), &amount);

        let mut info = read_stake(&env, &user);
        info.amount = info.amount + amount;
        env.storage().set(&DataKey::Stake(user.clone()), &info);

        let total = read_total_staked(&env);
        env.storage().set(&DataKey::TotalStaked, &(total + amount));

        events::emit_staked(&env, user, amount);
    }

    /// Request to unstake tokens. Tokens are released after the cooldown.
    pub fn request_unstake(env: Env, user: Address, amount: i128) {
        user.require_auth();
        if amount <= 0 {
            panic_with(env, StakingError::InvalidAmount);
        }
        let info = read_stake(&env, &user);
        if info.amount < amount {
            panic_with(env, StakingError::InsufficientStake);
        }

        let mut pending = read_pending_unstake(&env, &user);
        let available_at = env.ledger().sequence() + read_unstake_cooldown(&env);
        pending.amount = pending.amount + amount;
        pending.available_at = available_at;
        env.storage().set(&DataKey::PendingUnstake(user.clone()), &pending);

        // Reduce the user's active stake immediately.
        let mut info = info;
        info.amount = info.amount - amount;
        env.storage().set(&DataKey::Stake(user.clone()), &info);

        let total = read_total_staked(&env);
        env.storage().set(&DataKey::TotalStaked, &total.saturating_sub(amount));

        events::emit_unstake_requested(&env, user, amount, available_at);
    }

    /// Withdraw tokens after the unstake cooldown has elapsed.
    pub fn withdraw(env: Env, user: Address) {
        user.require_auth();
        let pending = read_pending_unstake(&env, &user);
        if pending.amount <= 0 {
            panic_with(env, StakingError::NoPendingUnstake);
        }
        if env.ledger().sequence() < pending.available_at {
            panic_with(env, StakingError::UnstakeNotAvailable);
        }

        let amount = pending.amount;
        env.storage().remove(&DataKey::PendingUnstake(user.clone()));

        let token = read_token(&env);
        TokenClient::new(&env, &token).transfer(
            env.current_contract().address(),
            user.clone(),
            &amount,
        );

        events::emit_withdrawn(&env, user, amount);
    }

    /// Claim accrued rewards for the caller.
    pub fn claim_reward(env: Env, user: Address) {
        user.require_auth();
        let info = read_stake(&env, &user);
        if info.amount <= 0 {
            panic_with(env, StakingError::InsufficientStake);
        }

        let mut reward = read_reward(&env, &user);
        accrue_reward(&env, &user, &info, &reward);
        let payout = reward.accrued;
        if payout <= 0 {
            panic_with(env, StakingError::NoReward);
        }
        reward.accrued = 0;
        env.storage().set(&DataKey::Reward(user.clone()), &reward);

        let token = read_token(&env);
        TokenClient::new(&env, &token).transfer(
            env.current_contract().address(),
            user.clone(),
            &payout,
        );

        events::emit_reward_paid(&env, user, payout);
    }

    /// Return the stake info for a user.
    pub fn stake_of(env: Env, user: Address) -> StakeInfo {
        read_stake(&env, &user)
    }

    /// Return the pending unstake for a user.
    pub fn pending_unstake_of(env: Env, user: Address) -> PendingUnstake {
        read_pending_unstake(&env, &user)
    }

    /// Return the accrued reward for a user.
    pub fn reward_of(env: Env, user: Address) -> i128 {
        let info = read_stake(&env, &user);
        let mut reward = read_reward(&env, &user);
        accrue_reward(&env, &user, &info, &reward);
        reward.accrued
    }

    /// Return total staked tokens.
    pub fn total_staked(env: Env) -> i128 {
        read_total_staked(&env)
    }
}

fn read_token(env: &Env) -> Address {
    env.storage()
        .get(&DataKey::Token)
        .unwrap_or_unwrap_or(panic_with(env.clone(), StakingError::NotInitialized))
}

fn read_total_staked(env: &Env) -> i128 {
    env.storage().get(&DataKey::TotalStaked).unwrap_or(0)
}

fn read_reward_rate_bps(env: &Env) -> i128 {
    env.storage()
        .get(&DataKey::RewardRateBps)
        .unwrap_or(DEFAULT_REWARD_RATE_BPS)
}

fn read_unstake_cooldown(env: &Env) -> u64 {
    env.storage()
        .get(&DataKey::UnstakeCooldown)
        .unwrap_or(UNSTAKE_COOLDOWN)
}

fn read_stake(env: &Env, user: &Address) -> StakeInfo {
    env.storage()
        .get(&DataKey::Stake(user.clone()))
        .unwrap_or(StakeInfo {
            amount: 0,
            staked_at: env.ledger().sequence(),
        })
}

fn read_pending_unstake(env: &Env, user: &Address) -> PendingUnstake {
    env.storage()
        .get(&DataKey::PendingUnstake(user.clone()))
        .unwrap_or(PendingUnstake {
            amount: 0,
            available_at: 0,
        })
}

fn read_reward(env: &Env, user: &Address) -> RewardInfo {
    env.storage()
        .get(&DataKey::Reward(user.clone()))
        .unwrap_or(RewardInfo {
            accrued: 0,
            last_accrual: env.ledger().sequence(),
        })
}

/// Accrue rewards for a user based on their active stake and time elapsed.
fn accrue_reward(env: &Env, user: &Address, info: &StakeInfo, reward: &mut RewardInfo) {
    let now = env.ledger().sequence();
    if now <= reward.last_accrual || info.amount <= 0 {
        reward.last_accrual = now;
        return;
    }
    let elapsed = now - reward.last_accrual;
    let rate_bps = read_reward_rate_bps(env);
    // reward = amount * rate_bps / 10000 * elapsed
    let numer = info.amount * rate_bps * (elapsed as i128);
    let earned = numer / 10000;
    reward.accrued = reward.accrued + earned;
    reward.last_accrual = now;
    env.storage().set(&DataKey::Reward(user.clone()), &reward);
}

/// Helper to convert an i128 amount to Val (keyed by a symbol).
pub fn amount_val(env: &Env, amount: i128) -> Val {
    amount.into_val()
}

#[cfg](test)]
mod test;
