#![cfg_attr(not(test), no_std)]

#[cfg(test)]
extern crate std;

mod errors;
mod events;
mod storage;

#[cfg(test)]
mod test;

use errors::StakingError;
use soroban_sdk::{contract, contractimpl, token, Address, Env};
use storage::StakingConfig;

/// Hard cap on `reward_rate` (units per ledger).
///
/// 1_000_000_000 = 10^9 — large enough for any realistic emission schedule
/// while still catching accidental values like `u64::MAX` that would overflow
/// future reward-accounting math.
pub const MAX_REWARD_RATE: u64 = 1_000_000_000;

#[contract]
pub struct Staking;

#[contractimpl]
impl Staking {
    /// Initialises the staking contract.
    ///
    /// Must be called exactly once.  Subsequent calls are rejected with
    /// [`StakingError::AlreadyInitialized`].
    ///
    /// # Arguments
    /// * `admin` — Address that may call [`Staking::set_reward_config`].
    /// * `lp_token` — SEP-41 LP token that users will lock in this contract.
    /// * `reward_token` — SEP-41 token distributed as staking rewards.
    /// * `reward_rate` — Emission rate in smallest token units per ledger.
    ///   Must be `<= MAX_REWARD_RATE` (10^9).
    ///
    /// # Authorization
    /// `admin` must authorize the call (`admin.require_auth()`).
    ///
    /// # Errors
    /// | Error                  | Condition                              |
    /// |------------------------|----------------------------------------|
    /// | `AlreadyInitialized`   | Contract already has config stored     |
    /// | `RewardRateTooHigh`    | `reward_rate > MAX_REWARD_RATE`        |
    pub fn initialize(
        env: Env,
        admin: Address,
        lp_token: Address,
        reward_token: Address,
        reward_rate: u64,
    ) -> Result<(), StakingError> {
        if storage::has_config(&env) {
            return Err(StakingError::AlreadyInitialized);
        }

        admin.require_auth();

        if reward_rate > MAX_REWARD_RATE {
            return Err(StakingError::RewardRateTooHigh);
        }

        let config = StakingConfig {
            lp_token: lp_token.clone(),
            reward_token: reward_token.clone(),
            reward_rate,
            admin: admin.clone(),
        };

        storage::set_config(&env, &config);
        // Total staked starts at zero — the DataKey::TotalStaked entry is
        // written here so callers always find a value rather than `None`.
        storage::set_total_staked(&env, 0i128);

        storage::extend_instance_ttl(&env);

        events::StakingEvents::initialized(&env, &lp_token, &reward_token, reward_rate, &admin);

        Ok(())
    }

    /// Locks `amount` LP tokens on behalf of `user`.
    ///
    /// Transfers `amount` from `user` to this contract and increments both the
    /// per-user balance and the global total-staked counter.
    ///
    /// # Authorization
    /// `user` must authorize the call.  The token `transfer` is a sub-invocation
    /// that the user's SEP-41 allowance satisfies automatically when the user
    /// signs the outer `stake` invocation.
    ///
    /// # Errors
    /// | Error            | Condition                               |
    /// |------------------|-----------------------------------------|
    /// | `NotInitialized` | `initialize` has not been called        |
    /// | `InvalidAmount`  | `amount <= 0`                           |
    pub fn stake(env: Env, user: Address, amount: i128) -> Result<(), StakingError> {
        let config = storage::get_config(&env).ok_or(StakingError::NotInitialized)?;

        user.require_auth();

        if amount <= 0 {
            return Err(StakingError::InvalidAmount);
        }

        // Pull LP tokens from the user into this contract.
        token::Client::new(&env, &config.lp_token).transfer(
            &user,
            env.current_contract_address(),
            &amount,
        );

        // Update per-user and global balances.
        let prev_user = storage::get_user_stake(&env, &user);
        let new_user = prev_user + amount;
        storage::set_user_stake(&env, &user, new_user);

        let prev_total = storage::get_total_staked(&env);
        let new_total = prev_total + amount;
        storage::set_total_staked(&env, new_total);

        storage::extend_instance_ttl(&env);

        events::StakingEvents::staked(&env, &user, amount, new_user, new_total);

        Ok(())
    }

    /// Withdraws `amount` locked LP tokens back to `user`.
    ///
    /// Decrements both the per-user balance and the global total-staked counter,
    /// then transfers the tokens from this contract to `user`.
    ///
    /// # Authorization
    /// `user` must authorize the call.
    ///
    /// # Errors
    /// | Error               | Condition                                  |
    /// |---------------------|--------------------------------------------|
    /// | `NotInitialized`    | `initialize` has not been called           |
    /// | `InvalidAmount`     | `amount <= 0`                              |
    /// | `InsufficientStake` | `amount > user's current stake`            |
    pub fn unstake(env: Env, user: Address, amount: i128) -> Result<(), StakingError> {
        let config = storage::get_config(&env).ok_or(StakingError::NotInitialized)?;

        user.require_auth();

        if amount <= 0 {
            return Err(StakingError::InvalidAmount);
        }

        let current = storage::get_user_stake(&env, &user);
        if amount > current {
            return Err(StakingError::InsufficientStake);
        }

        // Update balances before the transfer (checks-effects-interactions).
        let new_user = current - amount;
        storage::set_user_stake(&env, &user, new_user);

        let prev_total = storage::get_total_staked(&env);
        let new_total = prev_total - amount;
        storage::set_total_staked(&env, new_total);

        // Return the LP tokens to the user.
        token::Client::new(&env, &config.lp_token).transfer(
            &env.current_contract_address(),
            &user,
            &amount,
        );

        storage::extend_instance_ttl(&env);

        events::StakingEvents::unstaked(&env, &user, amount, new_user, new_total);

        Ok(())
    }

    /// Updates the reward token address and emission rate.
    ///
    /// Only the admin may call this.  Both fields are updated atomically so
    /// there is never a state where a new rate points at an old token or vice
    /// versa.
    ///
    /// # Authorization
    /// `admin` must authorize the call and must equal the stored admin address.
    ///
    /// # Errors
    /// | Error                | Condition                                  |
    /// |----------------------|--------------------------------------------|
    /// | `NotInitialized`     | `initialize` has not been called           |
    /// | `Unauthorized`       | `admin` ≠ stored admin                     |
    /// | `RewardRateTooHigh`  | `new_reward_rate > MAX_REWARD_RATE`        |
    pub fn set_reward_config(
        env: Env,
        admin: Address,
        new_reward_token: Address,
        new_reward_rate: u64,
    ) -> Result<(), StakingError> {
        let mut config = storage::get_config(&env).ok_or(StakingError::NotInitialized)?;

        admin.require_auth();

        if admin != config.admin {
            return Err(StakingError::Unauthorized);
        }

        if new_reward_rate > MAX_REWARD_RATE {
            return Err(StakingError::RewardRateTooHigh);
        }

        config.reward_token = new_reward_token.clone();
        config.reward_rate = new_reward_rate;
        storage::set_config(&env, &config);

        storage::extend_instance_ttl(&env);

        events::StakingEvents::reward_config_updated(&env, &new_reward_token, new_reward_rate);

        Ok(())
    }

    // -----------------------------------------------------------------------
    // View functions
    // -----------------------------------------------------------------------

    /// Returns the LP tokens staked by `user`, or `0` if the user has never
    /// staked.
    pub fn get_stake(env: Env, user: Address) -> i128 {
        storage::get_user_stake(&env, &user)
    }

    /// Returns the total LP tokens locked across all users.
    pub fn get_total_staked(env: Env) -> i128 {
        storage::get_total_staked(&env)
    }

    /// Returns the current [`StakingConfig`].
    ///
    /// # Errors
    /// Returns [`StakingError::NotInitialized`] when called before `initialize`.
    pub fn get_config(env: Env) -> Result<StakingConfig, StakingError> {
        storage::get_config(&env).ok_or(StakingError::NotInitialized)
    }
}
