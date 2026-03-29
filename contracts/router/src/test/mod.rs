use soroban_sdk::{contract, contractimpl, contracttype, Address, Env};

#[contract]
pub struct MockPair;

#[contracttype]
#[derive(Clone)]
pub enum MPKey {
    ReserveA,
    ReserveB,
    BurnAmountA,
    BurnAmountB,
    LiquidityToMint,
}

#[contractimpl]
impl MockPair {
    pub fn set_reserves(env: Env, reserve_a: i128, reserve_b: i128) {
        env.storage().instance().set(&MPKey::ReserveA, &reserve_a);
        env.storage().instance().set(&MPKey::ReserveB, &reserve_b);
    }

    pub fn get_reserves(env: Env) -> (i128, i128, u64) {
        let a: i128 = env.storage().instance().get(&MPKey::ReserveA).unwrap_or(0);

        let b: i128 = env.storage().instance().get(&MPKey::ReserveB).unwrap_or(0);

        (a, b, 0)
    }

    pub fn set_burn_amounts(env: Env, amount_a: i128, amount_b: i128) {
        env.storage().instance().set(&MPKey::BurnAmountA, &amount_a);
        env.storage().instance().set(&MPKey::BurnAmountB, &amount_b);
    }

    pub fn burn(env: Env, _to: Address) -> (i128, i128) {
        let a: i128 = env.storage().instance().get(&MPKey::BurnAmountA).unwrap_or(0);

        let b: i128 = env.storage().instance().get(&MPKey::BurnAmountB).unwrap_or(0);

        (a, b)
    }

    pub fn set_liquidity_to_mint(env: Env, liquidity: i128) {
        env.storage().instance().set(&MPKey::LiquidityToMint, &liquidity);
    }

    pub fn mint(env: Env, _to: Address) -> i128 {
        env.storage().instance().get(&MPKey::LiquidityToMint).unwrap_or(0)
    }

    pub fn swap(_env: Env, _amount_a_out: i128, _amount_b_out: i128, _to: Address) {}

    pub fn lp_token(_env: Env) -> Address {
        panic!("not needed for router unit tests")
    }

    pub fn get_current_fee_bps(_env: Env) -> u32 {
        30
    }
}
