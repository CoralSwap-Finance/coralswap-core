use soroban_sdk::{contracttype, Address, BytesN, String, Symbol, Val, Vec};

#[contracttype]
pub struct TokenMetadata {
    pub decimals: u32,
    pub name: String,
    pub symbol: String,
}

#[contracttype]
pub struct AllowanceEntry {
    pub amount: i128,
    pub expiration_ledger: u32,
}

/// A voting-power (or total-supply) checkpoint keyed by ledger sequence.
///
/// Checkpoints are stored in ascending `ledger` order within a `Vec`.  `value`
/// holds either an address's voting power or the total supply, depending on
/// which storage key the checkpoint list lives under.
#[contracttype]
pub struct Checkpoint {
    pub ledger: u32,
    pub value: i128,
}

// ─────────────────────────────────────────────────────────────────────────
// Governance proposals (issue #161)
// ─────────────────────────────────────────────────────────────────────────

/// What a governance proposal executes if it succeeds.
///
/// Stored alongside the proposal so that issue #162 (`execute_proposal`) can
/// replay it as `target::function(args)` once voting is over. Kept as a single
/// struct so `propose` takes one `execution_payload` argument, matching the
/// issue #161 signature.
#[contracttype]
#[derive(Clone, Debug)]
pub struct ExecutionPayload {
    /// Contract to invoke on execution.
    pub target: Address,
    /// Entry point to call on `target`.
    pub function: Symbol,
    /// Arguments forwarded to `target::function`.
    pub args: Vec<Val>,
}

/// Configurable governance parameters (issue #161).
///
/// Set by the admin via `set_governance_params`. When never set explicitly the
/// contract uses [`DEFAULT_VOTING_DELAY`] / [`DEFAULT_VOTING_PERIOD`] /
/// [`DEFAULT_PROPOSAL_THRESHOLD`] (see `lib.rs`), so the contract is usable out
/// of the box and tests do not need extra setup.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GovernanceParams {
    /// Ledgers between proposal creation and the start of voting (Pending).
    pub voting_delay: u32,
    /// Length of the voting window, in ledgers.
    pub voting_period: u32,
    /// Minimum voting power a proposer must hold to create a proposal.
    pub proposal_threshold: i128,
}

/// A governance proposal created via `propose` (issue #161).
///
/// Voting power for a cast vote is read from the proposal's `snapshot_ledger`
/// (the ledger the proposal was created in) using the checkpoint system, so a
/// voter can only vote with the power they held when the proposal was created —
/// not with power acquired afterwards to swing the vote.
#[contracttype]
#[derive(Clone, Debug)]
pub struct Proposal {
    pub id: u32,
    pub proposer: Address,
    /// Hash of the off-chain proposal description (identification only).
    pub description_hash: BytesN<32>,
    /// What to execute if the proposal succeeds (consumed by issue #162).
    pub payload: ExecutionPayload,
    /// Ledger at which the proposal was created — the voting-power snapshot.
    pub snapshot_ledger: u32,
    /// Ledger at which voting opens.
    pub start_ledger: u32,
    /// Ledger at which voting closes.
    pub end_ledger: u32,
    /// Aggregate voting power cast in favour.
    pub for_votes: i128,
    /// Aggregate voting power cast against.
    pub against_votes: i128,
}

/// Lifecycle of a governance proposal (issue #161).
#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ProposalState {
    /// Before voting opens (`current < start_ledger`).
    Pending,
    /// Voting is open (`start_ledger <= current <= end_ledger`).
    Active,
    /// Voting closed and "for" did not exceed "against".
    Defeated,
    /// Voting closed and "for" exceeded "against".
    Succeeded,
}

/// Receipt recorded when an address votes, used to prevent double voting.
#[contracttype]
#[derive(Clone, Debug)]
pub struct VoteReceipt {
    /// `true` = in favour, `false` = against.
    pub support: bool,
    /// Voting power the voter cast, taken at the proposal's snapshot ledger.
    pub weight: i128,
}

#[contracttype]
pub enum GovTokenKey {
    Balance(Address),
    Allowance(Address, Address),
    /// Who an address has delegated their voting power to.
    Delegate(Address),
    /// Sorted checkpoint list for an address's voting power.
    Checkpoints(Address),
    /// Sorted checkpoint list for the total supply.
    TotalSupplyCheckpoints,
    TotalSupply,
    Metadata,
    Admin,
    // ── Governance proposals (issue #161) ─────────────────────────────────
    /// A proposal, keyed by id.
    Proposal(u32),
    /// Next proposal id to assign (instance storage).
    ProposalCount,
    /// Records that an address has already voted on a proposal.
    VoteCast(u32, Address),
    /// Configurable governance parameters (instance storage).
    GovernanceParams,
}
