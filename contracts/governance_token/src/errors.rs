use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum GovTokenError {
    AlreadyInitialized = 200,
    NotInitialized = 201,
    Unauthorized = 202,
    InsufficientBalance = 203,
    InsufficientAllowance = 204,
    Overflow = 205,
    InvalidExpiration = 206,
    /// Metadata failed validation: decimals > 18 or name/symbol > 32 chars.
    InvalidMetadata = 207,
    /// Mint amount exceeds per-call limit.
    MintAmountTooLarge = 208,
    /// Total supply would exceed maximum allowed.
    TotalSupplyExceeded = 209,
    /// Requested ledger is in the future; historical queries only cover past ledgers.
    FutureLedger = 210,
    // ── Governance proposals (issue #161) ───────────────────────────────
    /// Proposer's voting power is below the proposal threshold.
    BelowProposalThreshold = 211,
    /// No proposal exists with the given id.
    ProposalNotFound = 212,
    /// A vote was cast outside the active voting window.
    VotingNotActive = 213,
    /// An address has already voted on this proposal.
    AlreadyVoted = 214,
    /// The voter had zero voting power at the proposal's snapshot ledger.
    NoVotingPower = 215,
    /// `set_governance_params` rejected an invalid combination (e.g. a
    /// voting period of zero, which would leave no voting window).
    InvalidGovernanceParams = 216,
}
