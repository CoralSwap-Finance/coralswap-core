use soroban_sdk::{contracttype, Address, Env};

use coralswap_shared::{INSTANCE_TTL_EXTEND_TO, INSTANCE_TTL_THRESHOLD};

// ---------------------------------------------------------------------------
// Storage key enum
// ---------------------------------------------------------------------------

/// All persistent keys used by the staking contract.
///
/// `DataKey` is serialised directly into Soroban instance storage.  Every
/// variant is documented with the type of value it maps to.
#[contracttype]
#[derive(Clone, Debug)]
pub enum DataKey {
    /// [`StakingConfig`] — written once on `initialize`, updated by admin.
    Config,
    /// `i128` — total LP tokens currently locked in the contract.
    TotalStaked,
    /// `i128` — per-user staked LP token balance, keyed by staker address.
    UserStake(Address),
    /// `i128` — cumulative rewards paid out per unit staked (reward-per-share).
    RewardPerShare,
    /// `u64` — ledger sequence at which rewards were last accrued.
    LastRewardLedger,
    /// `i128` — per-user `reward_per_share` value at last interaction, used to
    /// compute unclaimed pending rewards without double-paying.
    RewardDebt(Address),
}

// ---------------------------------------------------------------------------
// Config struct
// ---------------------------------------------------------------------------

/// Persistent configuration written by `initialize` and updated by the admin.
///
/// Stored under [`DataKey::Config`] in instance storage.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct StakingConfig {
    /// Address of the LP token that users lock in this contract.
    pub lp_token: Address,
    /// Address of the token distributed as staking rewards.
    pub reward_token: Address,
    /// Reward emission rate in smallest token units per ledger.
    ///
    /// Governance may update this value via `set_reward_config`.  It is
    /// intentionally stored as a raw rate here — the contract does NOT
    /// implement reward accumulation in this iteration (see issue scope).
    pub reward_rate: u64,
    /// Address with admin privileges (can update reward config).
    pub admin: Address,
}

// ---------------------------------------------------------------------------
// Accessors
// ---------------------------------------------------------------------------

pub fn get_config(env: &Env) -> Option<StakingConfig> {
    env.storage().instance().get(&DataKey::Config)
}

pub fn set_config(env: &Env, config: &StakingConfig) {
    env.storage().instance().set(&DataKey::Config, config);
}

pub fn has_config(env: &Env) -> bool {
    env.storage().instance().has(&DataKey::Config)
}

/// Returns the total LP tokens staked across all users.  Defaults to `0`
/// before the first stake.
pub fn get_total_staked(env: &Env) -> i128 {
    env.storage().instance().get(&DataKey::TotalStaked).unwrap_or(0i128)
}

pub fn set_total_staked(env: &Env, amount: i128) {
    env.storage().instance().set(&DataKey::TotalStaked, &amount);
}

/// Returns the staked balance for `user`.  Defaults to `0` for addresses that
/// have never staked.
pub fn get_user_stake(env: &Env, user: &Address) -> i128 {
    env.storage().instance().get(&DataKey::UserStake(user.clone())).unwrap_or(0i128)
}

pub fn set_user_stake(env: &Env, user: &Address, amount: i128) {
    env.storage().instance().set(&DataKey::UserStake(user.clone()), &amount);
}

/// Extends the instance TTL using the shared policy (3.5 days threshold →
/// 7 days bump).  Must be called at the end of every mutating entry point so
/// the contract stays alive across idle periods.
pub fn extend_instance_ttl(env: &Env) {
    env.storage().instance().extend_ttl(INSTANCE_TTL_THRESHOLD, INSTANCE_TTL_EXTEND_TO);
}

// ---------------------------------------------------------------------------
// Reward accrual state (issue #234)
// ---------------------------------------------------------------------------

/// Cumulative rewards paid out per unit staked, in smallest reward units.
///
/// Grows by `reward_rate * elapsed_ledgers / total_staked` each time rewards
/// are accrued, so every staker earns in proportion to their stake and how
/// long they held it.
pub fn get_reward_per_share(env: &Env) -> i128 {
    env.storage()
        .instance()
        .get(&DataKey::RewardPerShare)
        .unwrap_or(0)
}

pub fn set_reward_per_share(env: &Env, value: i128) {
    env.storage().instance().set(&DataKey::RewardPerShare, &value);
}

/// Ledger sequence at which [`get_reward_per_share`] was last advanced.
pub fn get_last_reward_ledger(env: &Env) -> u64 {
    env.storage()
        .instance()
        .get(&DataKey::LastRewardLedger)
        .unwrap_or(0)
}

pub fn set_last_reward_ledger(env: &Env, value: u64) {
    env.storage().instance().set(&DataKey::LastRewardLedger, &value);
}

/// `reward_per_share` value observed at `user`'s last staking interaction.
///
/// `user_stake * (reward_per_share - reward_debt[user])` is the user's pending
/// (not yet paid out) rewards.
pub fn get_reward_debt(env: &Env, user: &Address) -> i128 {
    env.storage()
        .instance()
        .get(&DataKey::RewardDebt(user.clone()))
        .unwrap_or(0)
}

pub fn set_reward_debt(env: &Env, user: &Address, value: i128) {
    env.storage()
        .instance()
        .set(&DataKey::RewardDebt(user.clone()), &value);
}

/// Accrue rewards earned since the last accrual and advance `last_reward_ledger`.
///
/// Rewards accrue only while `total_staked > 0`; an idle contract with no
/// stakers does not accumulate unclaimable rewards.
pub fn accrue_rewards(env: &Env) {
    let config = match get_config(env) {
        Some(c) => c,
        None => return,
    };

    let total = get_total_staked(env);
    let current = env.ledger().sequence();
    let last = get_last_reward_ledger(env);
    let elapsed = current.saturating_sub(last);

    if elapsed == 0 || total == 0 || config.reward_rate == 0 {
        set_last_reward_ledger(env, current);
        return;
    }

    let emitted = (config.reward_rate as i128)
        .checked_mul(elapsed as i128)
        .unwrap_or(i128::MAX);
    let per_share_increment = emitted / total;

    let mut rps = get_reward_per_share(env);
    rps = rps.saturating_add(per_share_increment);
    set_reward_per_share(env, rps);
    set_last_reward_ledger(env, current);
}
