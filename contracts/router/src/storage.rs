use soroban_sdk::{contracttype, Address, BytesN, Env, Vec};

#[contracttype]
pub enum DataKey {
    Factory,
    Hubs,
    Commit(Address),
    NonceUsed(Address, u64),
}

#[contracttype]
#[derive(Clone)]
pub struct CommitEntry {
    pub hash: BytesN<32>,
    pub ledger: u32,
}

pub fn set_factory(env: &Env, factory: &Address) {
    env.storage().instance().set(&DataKey::Factory, factory);
}

pub fn get_factory(env: &Env) -> Option<Address> {
    env.storage().instance().get(&DataKey::Factory)
}

pub fn set_hubs(env: &Env, hubs: &Vec<Address>) {
    env.storage().instance().set(&DataKey::Hubs, hubs);
}

pub fn get_hubs(env: &Env) -> Vec<Address> {
    env.storage().instance().get(&DataKey::Hubs).unwrap_or(Vec::new(env))
}

pub fn set_commit(env: &Env, sender: &Address, entry: &CommitEntry) {
    env.storage().instance().set(&DataKey::Commit(sender.clone()), entry);
}

pub fn get_commit(env: &Env, sender: &Address) -> Option<CommitEntry> {
    env.storage().instance().get(&DataKey::Commit(sender.clone()))
}

pub fn clear_commit(env: &Env, sender: &Address) {
    env.storage().instance().remove(&DataKey::Commit(sender.clone()));
}

pub fn is_nonce_used(env: &Env, sender: &Address, nonce: u64) -> bool {
    env.storage().instance().has(&DataKey::NonceUsed(sender.clone(), nonce))
}

pub fn set_nonce_used(env: &Env, sender: &Address, nonce: u64) {
    env.storage().instance().set(&DataKey::NonceUsed(sender.clone(), nonce), &true);
}
