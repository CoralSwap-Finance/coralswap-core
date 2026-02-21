use soroban_sdk::{contracttype, Address, BytesN, Env};

#[contracttype]
#[derive(Clone, Debug)]
pub struct FactoryStorage {
    pub signers: (Address, Address, Address),
    pub pair_wasm_hash: BytesN<32>,
    pub lp_token_wasm_hash: BytesN<32>,
    pub pair_count: u32,
    pub protocol_version: u32,
    pub paused: bool,
    pub fee_to: Option<Address>,
    pub fee_to_setter: Address,
}

#[contracttype]
#[derive(Clone, Debug)]
pub enum DataKey {
    Factory,
    Pair(Address, Address),
}

pub fn get_factory_storage(env: &Env) -> Option<FactoryStorage> {
    env.storage().instance().get(&DataKey::Factory)
}

pub fn set_factory_storage(env: &Env, storage: &FactoryStorage) {
    env.storage().instance().set(&DataKey::Factory, storage);
}

pub fn get_pair(env: &Env, token_a: Address, token_b: Address) -> Option<Address> {
    let key = DataKey::Pair(token_a, token_b);
    env.storage().instance().get(&key)
}

pub fn set_pair(env: &Env, token_a: Address, token_b: Address, pair: Address) {
    let key = DataKey::Pair(token_a, token_b);
    env.storage().instance().set(&key, &pair);
}

pub fn has_factory_storage(env: &Env) -> bool {
    env.storage().instance().has(&DataKey::Factory)
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct TimelockedAction {
    pub proposed_at: u64,
    pub delay_seconds: u64,
    pub action_id: u32,
}
