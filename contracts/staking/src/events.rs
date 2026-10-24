use soroban_sdk::{symbol_short, Address, Env};

/// Staking contract event helpers.
///
/// Each method emits one event.  Topics use `symbol_short!` (≤ 8 chars)
/// to stay within Soroban's topic-length limit.
pub struct StakingEvents;

// `deprecated`: Events::publish is superseded by the #[contractevent] macro;
// migration pending (same pattern as factory/events.rs).
#[allow(dead_code, deprecated)]
impl StakingEvents {
    /// Emitted when the contract is initialized.
    ///
    /// Topics: `["initialized"]`
    /// Data: `(lp_token, reward_token, reward_rate, admin)`
    pub fn initialized(
        env: &Env,
        lp_token: &Address,
        reward_token: &Address,
        reward_rate: u64,
        admin: &Address,
    ) {
        env.events().publish(
            (symbol_short!("init"),),
            (lp_token.clone(), reward_token.clone(), reward_rate, admin.clone()),
        );
    }

    /// Emitted when a user locks LP tokens.
    ///
    /// Topics: `["staked", user]`
    /// Data: `(amount, new_user_total, new_contract_total)`
    pub fn staked(env: &Env, user: &Address, amount: i128, user_total: i128, contract_total: i128) {
        env.events()
            .publish((symbol_short!("staked"), user.clone()), (amount, user_total, contract_total));
    }

    /// Emitted when a user withdraws locked LP tokens.
    ///
    /// Topics: `["unstaked", user]`
    /// Data: `(amount, new_user_total, new_contract_total)`
    pub fn unstaked(
        env: &Env,
        user: &Address,
        amount: i128,
        user_total: i128,
        contract_total: i128,
    ) {
        env.events().publish(
            (symbol_short!("unstaked"), user.clone()),
            (amount, user_total, contract_total),
        );
    }

    /// Emitted when the admin updates the reward token or rate.
    ///
    /// Topics: `["rwdupdate"]`
    /// Data: `(new_reward_token, new_reward_rate)`
    pub fn reward_config_updated(env: &Env, new_reward_token: &Address, new_reward_rate: u64) {
        env.events()
            .publish((symbol_short!("rwdupdate"),), (new_reward_token.clone(), new_reward_rate));
    }
}
