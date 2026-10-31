use soroban_sdk::{contracttype, Address, BytesN, Env, Symbol, Val, Vec};

// Shared factory TTL policy (issue #390). See coralswap-shared for cadence math.
use coralswap_shared::{
    FACTORY_INSTANCE_BUMP_AMOUNT as INSTANCE_BUMP_AMOUNT,
    FACTORY_INSTANCE_THRESHOLD as INSTANCE_LIFETIME_THRESHOLD,
};

#[contracttype]
#[derive(Clone, Debug)]
pub struct FactoryStorage {
    pub signers: Vec<Address>,
    pub pair_wasm_hash: BytesN<32>,
    pub concentrated_pair_wasm_hash: BytesN<32>,
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
    /// Dedicated pair counter kept in strict lockstep with `PairList`.
    /// A single `u32` read is significantly cheaper than deserialising the
    /// full `FactoryStorage` blob just to obtain the count.
    TotalPairs,
    /// Per-pair frozen flag.
    PairFrozen(Address),
    /// Fast boolean lookup for pair membership (issue #391).
    IsPair(Address),
    /// Fast boolean lookup for LP-token membership.
    IsLpToken(Address),
    /// Per-proposal storage keyed by monotonic `u32` ID (issue #162).
    Proposal(u32),
    /// Monotonic proposal counter; the next ID to assign equals the current value.
    ProposalCount,
}

pub fn is_pair_frozen(env: &Env, pair: &Address) -> bool {
    env.storage().instance().get(&DataKey::PairFrozen(pair.clone())).unwrap_or(false)
}

pub fn set_pair_frozen(env: &Env, pair: &Address, frozen: bool) {
    if frozen {
        env.storage().instance().set(&DataKey::PairFrozen(pair.clone()), &true);
    } else {
        env.storage().instance().remove(&DataKey::PairFrozen(pair.clone()));
    }
}

pub fn get_pair_list(env: &Env) -> Vec<Address> {
    env.storage().instance().get(&DataKey::PairList).unwrap_or(Vec::new(env))
}

pub fn set_pair_list(env: &Env, list: &Vec<Address>) {
    env.storage().instance().set(&DataKey::PairList, list);
}

// ---------------------------------------------------------------------------
// Total-pairs counter (kept in strict lockstep with PairList)
// ---------------------------------------------------------------------------

/// Returns the cached total-pairs counter. Returns `0` before any pair is
/// created (i.e. when the key has never been written).
pub fn get_total_pairs(env: &Env) -> u32 {
    env.storage().instance().get(&DataKey::TotalPairs).unwrap_or(0u32)
}

/// Persists the total-pairs counter. Must only be called immediately after
/// pushing to `PairList` so the two values remain in lockstep.
pub fn set_total_pairs(env: &Env, count: u32) {
    env.storage().instance().set(&DataKey::TotalPairs, &count);
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

/// Returns true if the address is a pair created by this factory.
pub fn is_pair(env: &Env, pair: &Address) -> bool {
    if env.storage().instance().has(&DataKey::IsPair(pair.clone())) {
        return env.storage().instance().get(&DataKey::IsPair(pair.clone())).unwrap_or(false);
    }
    // Fallback: check pair_list for backwards compatibility or tests
    let pair_list = get_pair_list(env);
    for i in 0..pair_list.len() {
        if pair_list.get(i).unwrap() == *pair {
            return true;
        }
    }
    false
}

/// Sets the pair membership flag for an address.
pub fn set_is_pair(env: &Env, pair: &Address, is_pair: bool) {
    let key = DataKey::IsPair(pair.clone());
    if is_pair {
        env.storage().instance().set(&key, &true);
    } else {
        env.storage().instance().remove(&key);
    }
}

pub fn is_lp_token(env: &Env, token: &Address) -> bool {
    env.storage().instance().get(&DataKey::IsLpToken(token.clone())).unwrap_or(false)
}

pub fn set_is_lp_token(env: &Env, token: &Address, is_lp_token: bool) {
    let key = DataKey::IsLpToken(token.clone());
    if is_lp_token {
        env.storage().instance().set(&key, &true);
    } else {
        env.storage().instance().remove(&key);
    }
}

/// Removes any per-pair fee override for `pair`, restoring the pair's dynamic
/// fee. A no-op if no override is currently stored.
///
/// `0` is never persisted as an override: a stored `0` is indistinguishable
/// from "set the swap fee to zero" at the read site, which would let a single
/// governance call silently zero a pair's fees (issue #311). `None` is the
/// single encoding for "no override".
pub fn remove_pair_fee_override(env: &Env, pair: &Address) {
    env.storage().instance().remove(&DataKey::PairFeeOverride(pair.clone()));
}

/// Extend instance storage TTL to keep contract alive.
pub fn extend_instance_ttl(env: &Env) {
    env.storage().instance().extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
}

// ---------------------------------------------------------------------------
// Governance proposal storage (issue #162)
// ---------------------------------------------------------------------------

/// Lifecycle state for a governance proposal.
///
/// The valid transitions are:
///   `Succeeded` → (queue_proposal) → `Queued` → (execute_proposal) → `Executed`
///
/// `Succeeded` is the initial state when a proposal is created: the multisig
/// quorum verified the proposal in the same transaction it was created, so it
/// starts as already-approved and waiting to be queued into the timelock.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum ProposalStatus {
    /// Quorum was met at creation; the proposal may now be queued.
    Succeeded,
    /// The proposal has been placed in the timelock queue.
    Queued,
    /// The proposal has been executed; no further action is possible.
    Executed,
}

/// A governance proposal that targets a cross-contract call.
///
/// # Storage
/// Stored in instance storage under `DataKey::Proposal(id)`.
/// For proposals with large `args` payloads, callers should be aware of
/// instance-storage size limits; modest calldata (a handful of scalar values)
/// fits comfortably.
#[contracttype]
#[derive(Clone, Debug)]
pub struct GovernanceProposal {
    /// Monotonic proposal identifier, assigned by `create_proposal`.
    pub id: u32,
    /// The contract to be called when the proposal is executed.
    pub target: Address,
    /// The function name to invoke on `target`.
    pub function: Symbol,
    /// The arguments to pass to `function`.
    pub args: Vec<Val>,
    /// The ledger sequence at which the proposal was created (and quorum met).
    pub proposed_at_ledger: u32,
    /// The ledger sequence at which the proposal was queued into the timelock,
    /// or `None` if not yet queued.
    pub queued_at_ledger: Option<u32>,
    /// Current lifecycle state.
    pub status: ProposalStatus,
}

/// Returns the current proposal counter (i.e. the *next* ID to assign).
pub fn get_proposal_count(env: &Env) -> u32 {
    env.storage().instance().get(&DataKey::ProposalCount).unwrap_or(0u32)
}

/// Persists the proposal counter.
pub fn set_proposal_count(env: &Env, count: u32) {
    env.storage().instance().set(&DataKey::ProposalCount, &count);
}

/// Loads a proposal by ID, returning `None` if it does not exist.
pub fn get_proposal(env: &Env, id: u32) -> Option<GovernanceProposal> {
    env.storage().instance().get(&DataKey::Proposal(id))
}

/// Persists a proposal.
pub fn set_proposal(env: &Env, proposal: &GovernanceProposal) {
    env.storage().instance().set(&DataKey::Proposal(proposal.id), proposal);
}
