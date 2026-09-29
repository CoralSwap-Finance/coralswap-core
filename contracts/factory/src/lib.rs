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
    contract, contractclient, contractimpl, Address, Bytes, BytesN, Env, String, Symbol, Vec,
};
use storage::FactoryStorage;

/// Maximum protocol-wide swap fee, in basis points (0.30%).
///
/// A protocol fee is only ever collected when a recipient is configured, so
/// this is the ceiling for `set_fee_to`.
pub const MAX_PROTOCOL_FEE_BPS: u32 = 30;

/// Maximum per-pair fee override, in basis points (1.00%).
pub const MAX_PAIR_FEE_BPS: u32 = 100;

#[contractclient(name = "PairClient")]
pub trait PairInterface {
    fn initialize(
        env: Env,
        factory: Address,
        token_a: Address,
        token_b: Address,
        lp_token: Address,
    ) -> Result<(), FactoryError>;
    fn lp_token(env: Env) -> Address;
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
    fn decimals(env: Env) -> u32;
    fn name(env: Env) -> String;
    fn symbol(env: Env) -> String;
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
            protocol_version: coralswap_shared::PROTOCOL_VERSION,
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
        // LP metadata standardization (issue #396):
        // 7 decimals matches SAC Stellar-asset precision; name and symbol are
        // deterministically derived from canonical token pair addresses:
        // Name: CORAL-SWAP-LP-<HEX8>, Symbol: CLP-<HEX8>.
        let (lp_name, lp_symbol) = coralswap_shared::derive_lp_metadata(&env, &token_0, &token_1);
        let lp_token_client = LpTokenClient::new(&env, &lp_token_address);
        lp_token_client
            .try_initialize(&pair_address, &coralswap_shared::LP_DECIMALS, &lp_name, &lp_symbol)
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
        storage::set_is_pair(&env, &pair_address, true);

        let pair_index = factory_storage.pair_count;
        factory_storage.pair_count += 1;
        storage::set_factory_storage(&env, &factory_storage);

        let mut pair_list = storage::get_pair_list(&env);
        pair_list.push_back(pair_address.clone());
        storage::set_pair_list(&env, &pair_list);
        storage::extend_instance_ttl(&env);

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

    /// Deterministically derives the pair address for `(token_a, token_b)`
    /// without deploying anything (issue #383).
    ///
    /// Mirrors the salt derivation in [`Factory::create_pair`] exactly:
    /// tokens are canonically sorted, the salt is
    /// `sha256(xdr(token_0) || xdr(token_1))`, and the address is derived
    /// from the current contract as deployer. Soroban contract addresses are
    /// deterministic in (deployer, salt), so the returned address equals the
    /// address `create_pair` will deploy (or has deployed) for the same
    /// token pair — enabling off-chain pool discovery without a factory call.
    ///
    /// Errors with `IdenticalTokens` when both arguments are equal, matching
    /// `create_pair`.
    pub fn get_pair_address(
        env: Env,
        token_a: Address,
        token_b: Address,
    ) -> Result<Address, FactoryError> {
        if token_a == token_b {
            return Err(FactoryError::IdenticalTokens);
        }

        let (token_0, token_1) =
            if token_a < token_b { (token_a, token_b) } else { (token_b, token_a) };

        let mut salt_data = Bytes::new(&env);
        salt_data.append(&token_0.to_xdr(&env));
        salt_data.append(&token_1.to_xdr(&env));
        let salt = env.crypto().sha256(&salt_data);

        Ok(env.deployer().with_current_contract(salt).deployed_address())
    }

    /// Returns the factory's protocol version (issue #383).
    ///
    /// Initialized to [`coralswap_shared::PROTOCOL_VERSION`] and bumped by
    /// one on every executed WASM upgrade.
    pub fn protocol_version(env: Env) -> Result<u32, FactoryError> {
        storage::get_factory_storage(&env)
            .map(|s| s.protocol_version)
            .ok_or(FactoryError::NotInitialized)
    }

    /// Returns true if `pair` is a valid pair contract created by this factory.
    ///
    /// Provides a single-call boolean view for routers, frontends, and off-chain
    /// indexers to verify pair authenticity without needing token addresses or
    /// parsing optional address collisions (issue #391).
    pub fn is_pair(env: Env, pair: Address) -> bool {
        storage::is_pair(&env, &pair)
    }

    /// Returns a paginated slice of pair addresses in exact storage creation order (FIFO).
    ///
    /// # Ordering & Pagination Contract (issue #387)
    /// Pairs are appended to internal storage (`PairList`) sequentially as they are
    /// created and are NEVER reordered or removed. This guarantees stable, deterministic
    /// pagination across arbitrary offsets and limits:
    /// - Index 0 is permanently the first pair ever created by this factory.
    /// - For any index `i`, `pair[i]` remains invariant as subsequent pairs are added.
    /// - Paginating with `offset = k * limit` guarantees complete coverage with zero
    ///   duplicates and zero skipped pairs.
    ///
    /// # Arguments
    /// * `offset` - 0-based starting index in creation order.
    /// * `limit` - Number of pairs to return (maximum 50).
    ///
    /// # Errors
    /// Returns [`FactoryError::LimitTooHigh`] if `limit > 50`.
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

        // Require a strict majority (`> n/2`) of the registered signers; see
        // `governance::quorum_threshold`.
        governance::verify_multisig(&env, &storage.signers, &signers)?;

        storage.paused = true;
        storage::set_factory_storage(&env, &storage);
        storage::extend_instance_ttl(&env);
        events::FactoryEvents::paused(&env);
        Ok(())
    }

    pub fn unpause(env: Env, signers: Vec<Address>) -> Result<(), FactoryError> {
        let mut storage = storage::get_factory_storage(&env).ok_or(FactoryError::NotInitialized)?;

        governance::verify_multisig(&env, &storage.signers, &signers)?;

        storage.paused = false;
        storage::set_factory_storage(&env, &storage);
        storage::extend_instance_ttl(&env);
        events::FactoryEvents::unpaused(&env);
        events::FactoryEvents::resumed(&env);
        Ok(())
    }

    /// Resumes protocol trading operations (alias for `unpause`).
    pub fn resume(env: Env, signers: Vec<Address>) -> Result<(), FactoryError> {
        Self::unpause(env, signers)
    }

    /// Heartbeat sync entrypoint for indexers and off-chain pollers.
    /// Emits a `sync` event with the current pause state and total pair count,
    /// and extends factory instance TTL.
    pub fn sync(env: Env) -> Result<(), FactoryError> {
        let storage = storage::get_factory_storage(&env).ok_or(FactoryError::NotInitialized)?;
        let total_pairs = storage::get_total_pairs(&env);
        storage::extend_instance_ttl(&env);
        events::FactoryEvents::sync(&env, storage.paused, total_pairs);
        Ok(())
    }

    /// Freezes an individual pair, preventing operations on that specific pair.
    pub fn freeze_pair(env: Env, signers: Vec<Address>, pair: Address) -> Result<(), FactoryError> {
        let storage = storage::get_factory_storage(&env).ok_or(FactoryError::NotInitialized)?;
        governance::verify_multisig(&env, &storage.signers, &signers)?;

        storage::set_pair_frozen(&env, &pair, true);
        storage::extend_instance_ttl(&env);
        events::FactoryEvents::pair_frozen(&env, &pair);
        Ok(())
    }

    /// Unfreezes an individual pair, restoring operations on that pair.
    pub fn unfreeze_pair(
        env: Env,
        signers: Vec<Address>,
        pair: Address,
    ) -> Result<(), FactoryError> {
        let storage = storage::get_factory_storage(&env).ok_or(FactoryError::NotInitialized)?;
        governance::verify_multisig(&env, &storage.signers, &signers)?;

        storage::set_pair_frozen(&env, &pair, false);
        storage::extend_instance_ttl(&env);
        events::FactoryEvents::pair_unfrozen(&env, &pair);
        Ok(())
    }

    /// Returns true if an individual pair is frozen.
    pub fn is_pair_frozen(env: Env, pair: Address) -> bool {
        storage::is_pair_frozen(&env, &pair)
    }

    /// Sets the protocol fee recipient and the protocol fee in basis points.
    ///
    /// # Semantics (issue #311)
    ///
    /// The pair of values is validated so that exactly one unambiguous encoding
    /// exists for each of the two protocol-fee states:
    ///
    /// | `fee_to`      | `fee_bps` | State                                              |
    /// |---------------|-----------|----------------------------------------------------|
    /// | `None`        | `0`       | Collection **disabled** (protocol takes nothing)     |
    /// | `Some(..)`    | `1..=30`  | Collection **enabled** at `fee_bps`                 |
    ///
    /// The two rejected combinations are both footguns that let the protocol be
    /// *configured* as if it were earning revenue while collecting nothing:
    ///
    /// - `None` + `fee_bps > 0` → `InvalidFeeRecipient`. Fees would be computed
    ///   with nowhere to send them.
    /// - `Some(fee_to)` + `fee_bps == 0` → `FeeDisabled`. A live recipient with
    ///   a zero rate looks enabled to governance and to monitoring, yet every
    ///   swap silently routes zero to it. Disabling is expressed *only* by
    ///   clearing `fee_to`, so "disabled" never needs a second encoding.
    ///
    /// This mirrors the Uniswap V2 sentinel (`feeTo == address(0)` disables the
    /// protocol fee) and keeps Balancer V3's property of emitting a
    /// fee-change event for every state transition, including a zeroing.
    ///
    /// # Events
    ///
    /// Emits `fee_to_set` and `protocol_fee_updated { old_fee_bps, new_fee_bps,
    /// fee_to }` on **every** successful change, so off-chain monitors can
    /// detect both the rate and the enabled/disabled transition from a single
    /// event.
    ///
    /// # Errors
    /// | Error                   | Condition                                    |
    /// |-------------------------|----------------------------------------------|
    /// | `NotInitialized`        | Factory storage absent                       |
    /// | `Unauthorized`          | `setter` is not the current `fee_to_setter`  |
    /// | `FeeTooHigh`            | `fee_bps > 30`                               |
    /// | `InvalidFeeRecipient`   | `fee_to == None && fee_bps > 0`              |
    /// | `FeeDisabled`           | `fee_to == Some(..) && fee_bps == 0`         |
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

        if fee_bps > MAX_PROTOCOL_FEE_BPS {
            return Err(FactoryError::FeeTooHigh);
        }

        match (&fee_to, fee_bps) {
            // A nonzero rate with nowhere to send the fees.
            (None, bps) if bps > 0 => return Err(FactoryError::InvalidFeeRecipient),
            // A live recipient wired to a zero rate: looks enabled, collects
            // nothing. Disabling must be expressed by clearing `fee_to`.
            (Some(_), 0) => return Err(FactoryError::FeeDisabled),
            _ => {}
        }

        let old_fee_bps = storage.fee_bps;
        storage.fee_to = fee_to.clone();
        storage.fee_bps = fee_bps;
        storage::set_factory_storage(&env, &storage);
        storage::extend_instance_ttl(&env);

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
        storage::extend_instance_ttl(&env);

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
    /// # Validation (issue #311)
    ///
    /// `fee_bps` must be in `0..=MAX_PAIR_FEE_BPS` (max 1%). Returns
    /// `FactoryError::FeeTooHigh` otherwise.
    ///
    /// `fee_bps == 0` **clears** the override rather than storing a zero rate:
    /// the pair falls back to its dynamic fee and `get_pair_fee_override`
    /// returns `None`. Persisting `0` would be read back as "charge nothing",
    /// letting a single governance call silently zero a pair's fees while the
    /// stored value still looked like a configured override. `None` is the only
    /// encoding for "no override", so the two states can never be confused.
    ///
    /// # Event
    /// Emits `pair_fee_override_set { pair, old_fee_bps, new_fee_bps, ledger }`
    /// on every successful call, including a clear (`new_fee_bps == 0`).
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

        if fee_bps > MAX_PAIR_FEE_BPS {
            return Err(FactoryError::FeeTooHigh);
        }

        let old_fee_bps = storage::get_pair_fee_override(&env, &pair).unwrap_or(0);

        if fee_bps == 0 {
            // Clear rather than persist a zero rate: a stored `0` is
            // indistinguishable from "charge nothing" at the read site.
            storage::remove_pair_fee_override(&env, &pair);
        } else {
            storage::set_pair_fee_override(&env, &pair, fee_bps);
        }

        storage::extend_instance_ttl(&env);

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
    /// their effective fee: `None` (or, defensively, `Some(0)`) means "use the
    /// pair's dynamic fee"; any `Some(bps)` with `bps > 0` replaces it.
    pub fn get_pair_fee_override(env: Env, pair: Address) -> Option<u32> {
        storage::get_pair_fee_override(&env, &pair)
    }

    /// Records protocol fees collected by a pair.
    ///
    /// The pair computes the protocol's share of the swap fee, transfers it to
    /// `fee_to`, and then calls this method so the factory can keep a
    /// per-token balance and emit an event. Only pairs deployed by this
    /// factory are allowed to record fees.
    pub fn deposit_protocol_fee(
        env: Env,
        pair: Address,
        token: Address,
        amount: i128,
    ) -> Result<(), FactoryError> {
        pair.require_auth();
        let factory_storage =
            storage::get_factory_storage(&env).ok_or(FactoryError::NotInitialized)?;

        if factory_storage.fee_to.is_none() || factory_storage.fee_bps == 0 {
            return Err(FactoryError::InvalidFeeRecipient);
        }

        if !storage::is_pair(&env, &pair) {
            return Err(FactoryError::Unauthorized);
        }

        if amount <= 0 {
            return Err(FactoryError::Unauthorized);
        }

        let mut key = Bytes::new(&env);
        key.append(&Symbol::new(&env, "protocol_fee_balance").to_xdr(&env));
        key.append(&token.clone().to_xdr(&env));
        let balance: i128 = env.storage().instance().get(&key).unwrap_or(0);
        env.storage().instance().set(&key, &(balance + amount));

        #[allow(deprecated)]
        env.events().publish((Symbol::new(&env, "protocol_fee_collected"), token), (amount,));

        storage::extend_instance_ttl(&env);
        Ok(())
    }

    /// Returns the total protocol fees accumulated for `token`.
    pub fn get_protocol_fee_balance(env: Env, token: Address) -> i128 {
        let mut key = Bytes::new(&env);
        key.append(&Symbol::new(&env, "protocol_fee_balance").to_xdr(&env));
        key.append(&token.to_xdr(&env));
        env.storage().instance().get(&key).unwrap_or(0)
    }

    /// Returns the protocol fee configured on the factory, in basis points.
    pub fn fee_bps(env: Env) -> u32 {
        storage::get_factory_storage(&env).map(|s| s.fee_bps).unwrap_or(0)
    }

    pub fn get_fee_bps(env: Env) -> u32 {
        Self::fee_bps(env)
    }

    pub fn fee_to(env: Env) -> Option<Address> {
        storage::get_factory_storage(&env).map(|s| s.fee_to).unwrap_or(None)
    }

    pub fn get_fee_to(env: Env) -> Option<Address> {
        Self::fee_to(env)
    }

    pub fn fee_to_setter(env: Env) -> Option<Address> {
        storage::get_factory_storage(&env).map(|s| s.fee_to_setter)
    }

    /// Returns the WASM hash new pair contracts are deployed from.
    pub fn get_pair_wasm_hash(env: Env) -> Result<BytesN<32>, FactoryError> {
        storage::get_factory_storage(&env)
            .map(|s| s.pair_wasm_hash)
            .ok_or(FactoryError::NotInitialized)
    }

    /// Returns the WASM hash new LP token contracts are deployed from.
    pub fn get_lp_token_wasm_hash(env: Env) -> Result<BytesN<32>, FactoryError> {
        storage::get_factory_storage(&env)
            .map(|s| s.lp_token_wasm_hash)
            .ok_or(FactoryError::NotInitialized)
    }

    pub fn is_paused(env: Env) -> bool {
        storage::get_factory_storage(&env).map(|s| s.paused).unwrap_or(false)
    }

    /// Proposes a WASM upgrade. Gated by multisig (strict majority, `> n/2`).
    pub fn propose_upgrade(
        env: Env,
        signers: Vec<Address>,
        new_wasm_hash: BytesN<32>,
    ) -> Result<(), FactoryError> {
        let factory_storage =
            storage::get_factory_storage(&env).ok_or(FactoryError::NotInitialized)?;
        governance::verify_multisig(&env, &factory_storage.signers, &signers)?;
        upgrade::propose_upgrade(&env, new_wasm_hash)?;
        storage::extend_instance_ttl(&env);
        Ok(())
    }

    /// Executes a pending upgrade after the 72-hour timelock has elapsed.
    pub fn execute_upgrade(env: Env) -> Result<(), FactoryError> {
        upgrade::execute_upgrade(&env)?;
        storage::extend_instance_ttl(&env);
        Ok(())
    }

    /// Cancels a pending upgrade. Gated by multisig.
    pub fn cancel_upgrade(env: Env, signers: Vec<Address>) -> Result<(), FactoryError> {
        let factory_storage =
            storage::get_factory_storage(&env).ok_or(FactoryError::NotInitialized)?;
        governance::verify_multisig(&env, &factory_storage.signers, &signers)?;
        upgrade::cancel_upgrade(&env)?;
        storage::extend_instance_ttl(&env);
        Ok(())
    }
}
