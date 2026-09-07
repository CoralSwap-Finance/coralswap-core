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
use soroban_sdk::{
    contract, contractclient, contractimpl, Address, Bytes, BytesN, Env, String, Vec,
};
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

#[contractclient(name = "LpTokenClient")]
pub trait LpTokenInterface {
    fn initialize(
        env: Env,
        admin: Address,
        decimals: u32,
        name: String,
        symbol: String,
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
            fee_bps: 0,
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
            .deploy_v2(factory_storage.pair_wasm_hash.clone(), ());

        // 2. Deploy LP Token
        let mut lp_salt_data = Bytes::new(&env);
        lp_salt_data.append(&pair_address.clone().to_xdr(&env));
        let lp_salt = env.crypto().sha256(&lp_salt_data);

        let lp_token_address = env
            .deployer()
            .with_current_contract(lp_salt)
            .deploy_v2(factory_storage.lp_token_wasm_hash.clone(), ());

        // The pair is the sole LP token minter. Initialize the freshly
        // deployed token before exposing the pair so the first liquidity mint
        // cannot fail with an uninitialized-token error.
        let lp_token_client = LpTokenClient::new(&env, &lp_token_address);
        lp_token_client
            .try_initialize(
                &pair_address,
                &7,
                &String::from_str(&env, "Coral LP"),
                &String::from_str(&env, "CLP"),
            )
            .map_err(|_| FactoryError::NotInitialized)?
            .map_err(|_| FactoryError::NotInitialized)?;

        // 3. Initialize Pair — propagate any error; do NOT store if this fails
        // `try_initialize` returns a nested `Result`: outer layer covers
        // invocation errors, inner one carries the pair's own error. Both
        // must fail the call, otherwise an uninitialized pair gets stored.
        let pair_client = PairClient::new(&env, &pair_address);
        pair_client
            .try_initialize(&env.current_contract_address(), &token_0, &token_1, &lp_token_address)
            .map_err(|_| FactoryError::NotInitialized)?
            .map_err(|_| FactoryError::NotInitialized)?;

        // 4. Store pair — only reached when initialize() succeeded
        storage::set_pair(&env, token_0.clone(), token_1.clone(), pair_address.clone());
        storage::set_pair(&env, token_1.clone(), token_0.clone(), pair_address.clone());

        let pair_index = factory_storage.pair_count;
        factory_storage.pair_count += 1;
        storage::set_factory_storage(&env, &factory_storage);

        let mut pair_list = storage::get_pair_list(&env);
        pair_list.push_back(pair_address.clone());
        storage::set_pair_list(&env, &pair_list);

        // Keep total_pairs counter in strict lockstep with the list length.
        // This is the canonical source of truth for off-chain pagination:
        // a single u32 read vs a full Vec deserialisation.
        storage::set_total_pairs(&env, pair_list.len());

        // 5. Emit event
        events::FactoryEvents::pair_created(&env, &token_0, &token_1, &pair_address, pair_index);

        Ok(pair_address)
    }

    pub fn get_pair(env: Env, token_a: Address, token_b: Address) -> Option<Address> {
        storage::get_pair(&env, token_a, token_b)
    }

    pub fn get_all_pairs(env: Env, offset: u32, limit: u32) -> Result<Vec<Address>, FactoryError> {
        if limit > 50 {
            return Err(FactoryError::LimitTooHigh);
        }
        let pair_list = storage::get_pair_list(&env);
        let mut result = Vec::new(&env);
        let total = pair_list.len();

        let start = offset;
        let end = (offset + limit).min(total);
        if start < total {
            for i in start..end {
                result.push_back(pair_list.get(i).unwrap());
            }
        }
        Ok(result)
    }

    pub fn get_pair_count(env: Env) -> u32 {
        let storage = storage::get_factory_storage(&env);
        storage.map(|s| s.pair_count).unwrap_or(0)
    }

    /// Returns the total number of pairs ever created by this factory.
    ///
    /// This counter is stored under its own dedicated storage key and updated
    /// in strict lockstep with the `PairList` inside [`Factory::create_pair`].
    /// Reading it costs a single instance-storage lookup (one `u32`) rather
    /// than deserialising the full `FactoryStorage` blob, making it the
    /// preferred source for off-chain "count-before-batch" pagination.
    pub fn total_pairs(env: Env) -> u32 {
        storage::get_total_pairs(&env)
    }

    pub fn pause(env: Env, signers: Vec<Address>) -> Result<(), FactoryError> {
        let mut storage = storage::get_factory_storage(&env).ok_or(FactoryError::NotInitialized)?;

        // Require a majority (threshold = ceil(n/2)) of the registered signers.
        let threshold = storage.signers.len().div_ceil(2);
        governance::verify_multisig(&env, &signers, threshold)?;

        // Require that at least one of the (already auth-verified) provided
        // signers is a registered signer. `verify_multisig` already called
        // `require_auth()` on every provided signer above, so this is a
        // membership check only — a second `require_auth()` on the same
        // address here would be a redundant re-authorization within the same
        // call frame, which soroban-sdk rejects.
        signers.iter().find(|s| storage.signers.contains(s)).ok_or(FactoryError::Unauthorized)?;

        storage.paused = true;
        storage::set_factory_storage(&env, &storage);
        events::FactoryEvents::paused(&env);
        Ok(())
    }

    pub fn unpause(env: Env, signers: Vec<Address>) -> Result<(), FactoryError> {
        let mut storage = storage::get_factory_storage(&env).ok_or(FactoryError::NotInitialized)?;

        let threshold = storage.signers.len().div_ceil(2);
        governance::verify_multisig(&env, &signers, threshold)?;

        // See the matching comment in `pause()` — membership check only,
        // `verify_multisig` already required auth from every provided signer.
        signers.iter().find(|s| storage.signers.contains(s)).ok_or(FactoryError::Unauthorized)?;

        storage.paused = false;
        storage::set_factory_storage(&env, &storage);
        events::FactoryEvents::unpaused(&env);
        Ok(())
    }

    pub fn set_fee_to(
        env: Env,
        setter: Address,
        fee_to: Option<Address>,
        fee_bps: u32,
    ) -> Result<(), FactoryError> {
        let mut storage = storage::get_factory_storage(&env).ok_or(FactoryError::NotInitialized)?;

        setter.require_auth();

        if setter != storage.fee_to_setter {
            return Err(FactoryError::Unauthorized);
        }

        if fee_bps > 30 {
            return Err(FactoryError::FeeTooHigh);
        }

        if fee_to.is_none() && fee_bps > 0 {
            return Err(FactoryError::InvalidFeeRecipient);
        }

        let old_fee_bps = storage.fee_bps;
        storage.fee_to = fee_to.clone();
        storage.fee_bps = fee_bps;
        storage::set_factory_storage(&env, &storage);

        events::FactoryEvents::fee_to_set(&env, &fee_to);
        events::FactoryEvents::protocol_fee_updated(&env, old_fee_bps, fee_bps, &fee_to);

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

    /// Sets a per-pair fee override (issue #132).
    ///
    /// High-volume pairs (e.g. USDC/EURC) can be assigned a lower fee than the
    /// protocol-wide default without redeploying the pool. The override is
    /// read by the pair on the very next swap — no migration required.
    ///
    /// # Authorization
    /// Caller must be the current `fee_to_setter` address.
    ///
    /// # Validation
    /// `fee_bps` must be in `0..=100` (max 1%). Returns
    /// `FactoryError::FeeTooHigh` otherwise. A value of `0` is allowed and
    /// effectively clears the override on the next swap.
    ///
    /// # Event
    /// Emits `PairFeeOverrideEvent { pair, old_fee_bps, new_fee_bps, ledger }`.
    pub fn set_pair_fee(
        env: Env,
        setter: Address,
        pair: Address,
        fee_bps: u32,
    ) -> Result<(), FactoryError> {
        let storage = storage::get_factory_storage(&env).ok_or(FactoryError::NotInitialized)?;

        setter.require_auth();

        if setter != storage.fee_to_setter {
            return Err(FactoryError::Unauthorized);
        }

        if fee_bps > 100 {
            return Err(FactoryError::FeeTooHigh);
        }

        let old_fee_bps = storage::get_pair_fee_override(&env, &pair).unwrap_or(0);
        storage::set_pair_fee_override(&env, &pair, fee_bps);

        events::FactoryEvents::pair_fee_override_set(
            &env,
            &pair,
            old_fee_bps,
            fee_bps,
            env.ledger().sequence(),
        );

        Ok(())
    }

    /// Returns the per-pair fee override in basis points, or `None` if no
    /// override has been set. Pairs consult this on every swap to determine
    /// their effective fee.
    pub fn get_pair_fee_override(env: Env, pair: Address) -> Option<u32> {
        storage::get_pair_fee_override(&env, &pair)
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

    /// Proposes a WASM upgrade. Gated by multisig (threshold = ceil(n/2)).
    pub fn propose_upgrade(
        env: Env,
        signers: Vec<Address>,
        new_wasm_hash: BytesN<32>,
    ) -> Result<(), FactoryError> {
        let factory_storage =
            storage::get_factory_storage(&env).ok_or(FactoryError::NotInitialized)?;
        let threshold = factory_storage.signers.len().div_ceil(2);
        governance::verify_multisig(&env, &signers, threshold)?;
        upgrade::propose_upgrade(&env, new_wasm_hash)
    }

    /// Executes a pending upgrade after the 72-hour timelock has elapsed.
    pub fn execute_upgrade(env: Env) -> Result<(), FactoryError> {
        upgrade::execute_upgrade(&env)
    }

    /// Cancels a pending upgrade. Gated by multisig.
    pub fn cancel_upgrade(env: Env, signers: Vec<Address>) -> Result<(), FactoryError> {
        let factory_storage =
            storage::get_factory_storage(&env).ok_or(FactoryError::NotInitialized)?;
        let threshold = factory_storage.signers.len().div_ceil(2);
        governance::verify_multisig(&env, &signers, threshold)?;
        upgrade::cancel_upgrade(&env)
    }
}
