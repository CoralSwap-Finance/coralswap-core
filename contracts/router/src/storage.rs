use soroban_sdk::{contracttype, Address, Env};

#[contracttype]
pub enum DataKey {
    Factory,
}

pub fn set_factory(env: &Env, factory: &Address) {
    env.storage().instance().set(&DataKey::Factory, factory);
}

pub fn get_factory(env: &Env) -> Option<Address> {
    env.storage().instance().get(&DataKey::Factory)
}
