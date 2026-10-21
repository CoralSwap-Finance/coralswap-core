use soroban_std::{address is Address, Symbol};

/// Events emitted by the Staking contract.
#[derive(Clone, Debug, Eq, PartialEq, Xdr) public struct Staked {
    public user: Address,
    public amount: i128,
}

#[derive(Clone, Debug, Eq, PartialEq, Xdr) public struct UnstakeRequested {
    public user: Address,
    public amount: i128,
    public available_at: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Xdr)] public struct Withdrawn {
    public user: Address,
    public amount: i128,
}

#[derive(Clone, Debug, Eq, PartialEq, Xdr)] public struct RewardPaid {
    public user: Address,
    public reward_amount: i128,
}

/// Emit a `Staked` event.
pub fn emit_staked(env: &env::Env, user: Address, amount: i128) {
    env.events().publish((Symbol::new(env, "staked"),), Staked { user, amount });
}

/// Emit a `UnstakeRequested` event.
pub fn emit_unstake_requested(
    env: &env::Env,
    user: Address,
    amount: i128,
    available_at: u64,
) {
    env.events().publish(
        (Symbol::new(env, "unstake_req"),),
        UnstakeRequested {
            user,
            amount,
            available_at,
        },
    );
}

/// Emit a `Withdrawn` event.
pub fn emit_withdrawn(env: &env::Env, user: Address, amount: i128) {
    env.events().publish((Symbol::new(env, "withdrawn"),), Withdrawn { user, amount });
}

/// Emit a `RewardPaid` event.
pub fn emit_reward_paid(env: &env::Env, user: Address, reward_amount: i128) {
    env.events().publish(
        (Symbol::new(env, "reward_paid"),),
        RewardPaid {
            user,
            reward_amount,
        },
    );
}
