#![cfg_attr(not(test), no_std)]

#[cfg(test)]
extern crate std;

mod errors;
mod events;
mod governance;
mod storage;
mod upgrade;

#[cfg(test)]
mod test;

use errors::FactoryError;
use soroban_sdk::xdr::ToXdr;
use soroban_sdk::{contract, contractclient, contractimpl, Address, Bytes, BytesN, Env, Vec};
use storage::FactoryStorage;

#[contractclient(name = "PairClient")]
pub trait PairInterface {
    fn initialize(
        env: Env,
        factory: Address,
        token_a: Address,
        token_b: Address,
        lp_token: Address,
    ) -> Result<(), FactoryError>;
}

#[contract]
pub struct Factory;

#[contractimpl]
impl Factory {
    pub fn initialize(
        env: Env,
        signers: Vec<Address>,
        pair_wasm_hash: BytesN<32>,
        lp_token_wasm_hash: BytesN<32>,
        fee_to_setter: Address,
    ) -> Result<(), FactoryError> {
        // Double-init guard
        if storage::has_factory_storage(&env) {
            return Err(FactoryError::AlreadyInitialized);
        }

        // Validate signers: must have between 1 and 10 (inclusive)
        let signer_count = signers.len();
        if !(1..=10).contains(&signer_count) {
            return Err(FactoryError::InvalidSignerCount);
        }

        let factory_storage = FactoryStorage {
            signers,
            pair_wasm_hash,
            lp_token_wasm_hash,
            pair_count: 0,
            protocol_version: 1,
            paused: false,
            fee_to: None,
            fee_to_setter,
        };

        storage::set_factory_storage(&env, &factory_storage);

        // Extend instance TTL to keep contract alive
        storage::extend_instance_ttl(&env);

        Ok(())
    }

    pub fn create_pair(
        env: Env,
        token_a: Address,
        token_b: Address,
    ) -> Result<Address, FactoryError> {
        if token_a == token_b {
            return Err(FactoryError::IdenticalTokens);
        }

        let (token_0, token_1) =
            if token_a < token_b { (token_a, token_b) } else { (token_b, token_a) };

        if storage::get_pair(&env, token_0.clone(), token_1.clone()).is_some() {
            return Err(FactoryError::PairExists);
        }

        let mut factory_storage =
            storage::get_factory_storage(&env).ok_or(FactoryError::NotInitialized)?;

        if factory_storage.paused {
            return Err(FactoryError::ProtocolPaused);
        }

        // 1. Deploy Pair
        let mut salt_data = Bytes::new(&env);
        salt_data.append(&token_0.clone().to_xdr(&env));
        salt_data.append(&token_1.clone().to_xdr(&env));
        let salt = env.crypto().sha256(&salt_data);

        let pair_address = env
            .deployer()
            .with_current_contract(salt.clone())
            .deploy(factory_storage.pair_wasm_hash.clone());

        // 2. Deploy LP Token
        let mut lp_salt_data = Bytes::new(&env);
        lp_salt_data.append(&pair_address.clone().to_xdr(&env));
        let lp_salt = env.crypto().sha256(&lp_salt_data);

        let lp_token_address = env
            .deployer()
            .with_current_contract(lp_salt)
            .deploy(factory_storage.lp_token_wasm_hash.clone());

        // 3. Initialize Pair
        let pair_client = PairClient::new(&env, &pair_address);
        pair_client.initialize(
            &env.current_contract_address(),
            &token_0,
            &token_1,
            &lp_token_address,
        );

        // 4. Store pair
        storage::set_pair(&env, token_0.clone(), token_1.clone(), pair_address.clone());
        storage::set_pair(&env, token_1.clone(), token_0.clone(), pair_address.clone());

        let pair_index = factory_storage.pair_count;
        factory_storage.pair_count += 1;
        storage::set_factory_storage(&env, &factory_storage);

        // 5. Emit event
        events::FactoryEvents::pair_created(&env, &token_0, &token_1, &pair_address, pair_index);

        Ok(pair_address)
    }

    pub fn get_pair(env: Env, token_a: Address, token_b: Address) -> Option<Address> {
        storage::get_pair(&env, token_a, token_b)
    }

    pub fn pause(env: Env, _signers: Vec<Address>) -> Result<(), FactoryError> {
        let mut storage = storage::get_factory_storage(&env).ok_or(FactoryError::NotInitialized)?;
        // TODO: Auth check for signers
        storage.paused = true;
        storage::set_factory_storage(&env, &storage);
        events::FactoryEvents::paused(&env);
        Ok(())
    }

    pub fn unpause(env: Env, _signers: Vec<Address>) -> Result<(), FactoryError> {
        let mut storage = storage::get_factory_storage(&env).ok_or(FactoryError::NotInitialized)?;
        // TODO: Auth check for signers
        storage.paused = false;
        storage::set_factory_storage(&env, &storage);
        events::FactoryEvents::unpaused(&env);
        Ok(())
    }

    pub fn set_fee_to(
        env: Env,
        setter: Address,
        fee_to: Option<Address>,
    ) -> Result<(), FactoryError> {
        let mut storage = storage::get_factory_storage(&env).ok_or(FactoryError::NotInitialized)?;

        setter.require_auth();

        if setter != storage.fee_to_setter {
            return Err(FactoryError::Unauthorized);
        }

        storage.fee_to = fee_to.clone();
        storage::set_factory_storage(&env, &storage);

        events::FactoryEvents::fee_to_set(&env, &fee_to);

        Ok(())
    }

    pub fn set_fee_to_setter(
        env: Env,
        setter: Address,
        new_setter: Address,
    ) -> Result<(), FactoryError> {
        let mut storage = storage::get_factory_storage(&env).ok_or(FactoryError::NotInitialized)?;

        setter.require_auth();

        if setter != storage.fee_to_setter {
            return Err(FactoryError::Unauthorized);
        }

        storage.fee_to_setter = new_setter.clone();
        storage::set_factory_storage(&env, &storage);

        events::FactoryEvents::fee_to_setter_set(&env, &new_setter);

        Ok(())
    }

    pub fn fee_to(env: Env) -> Option<Address> {
        storage::get_factory_storage(&env).map(|s| s.fee_to).unwrap_or(None)
    }

    pub fn fee_to_setter(env: Env) -> Option<Address> {
        storage::get_factory_storage(&env).map(|s| s.fee_to_setter)
    }

    pub fn is_paused(env: Env) -> bool {
        storage::get_factory_storage(&env).map(|s| s.paused).unwrap_or(false)
    }
}
