use soroban_sdk::{contracttype, Address, BytesN, Env, Vec};

const INSTANCE_LIFETIME_THRESHOLD: u32 = 17280; // ~1 day in 5s ledgers
const INSTANCE_BUMP_AMOUNT: u32 = 518400; // ~30 days in 5s ledgers

#[contracttype]
#[derive(Clone, Debug)]
pub struct FactoryStorage {
    pub signers: Vec<Address>,
    pub pair_wasm_hash: BytesN<32>,
    pub lp_token_wasm_hash: BytesN<32>,
    pub pair_count: u32,
    pub protocol_version: u32,
    pub paused: bool,
    pub fee_to: Option<Address>,
    pub fee_to_setter: Address,
    pub fee_bps: u32,
}

#[contracttype]
#[derive(Clone, Debug)]
pub enum DataKey {
    Factory,
    Pair(Address, Address),
    PendingUpgrade,
    PairList,
    /// Per-pair fee override (see `Factory::set_pair_fee`, issue #132).
    /// Stored as `u32` basis points; absence means "use the pair's dynamic fee".
    PairFeeOverride(Address),
}

pub fn get_pair_list(env: &Env) -> Vec<Address> {
    env.storage().instance().get(&DataKey::PairList).unwrap_or(Vec::new(env))
}

pub fn set_pair_list(env: &Env, list: &Vec<Address>) {
    env.storage().instance().set(&DataKey::PairList, list);
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct PendingUpgrade {
    pub new_wasm_hash: BytesN<32>,
    pub proposed_at_ledger: u32,
}

pub fn get_pending_upgrade(env: &Env) -> Option<PendingUpgrade> {
    env.storage().instance().get(&DataKey::PendingUpgrade)
}

pub fn set_pending_upgrade(env: &Env, proposal: &PendingUpgrade) {
    env.storage().instance().set(&DataKey::PendingUpgrade, proposal);
}

pub fn remove_pending_upgrade(env: &Env) {
    env.storage().instance().remove(&DataKey::PendingUpgrade);
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

// ---------------------------------------------------------------------------
// Per-pair fee override (issue #132)
// ---------------------------------------------------------------------------

/// Returns the per-pair fee override in basis points, or `None` if no override
/// has been set for `pair`.
pub fn get_pair_fee_override(env: &Env, pair: &Address) -> Option<u32> {
    env.storage().instance().get(&DataKey::PairFeeOverride(pair.clone()))
}

/// Stores the per-pair fee override in basis points. Callers are responsible
/// for validating that `fee_bps <= 100` (enforced by `Factory::set_pair_fee`).
pub fn set_pair_fee_override(env: &Env, pair: &Address, fee_bps: u32) {
    env.storage().instance().set(&DataKey::PairFeeOverride(pair.clone()), &fee_bps);
}

/// Extend instance storage TTL to keep contract alive.
pub fn extend_instance_ttl(env: &Env) {
    env.storage().instance().extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
}
