use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum FactoryError {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    PairExists = 3,
    InvalidSignerCount = 4,
    InsufficientSignatures = 5,
    TimelockNotExpired = 6,
    ProtocolPaused = 7,
    IdenticalTokens = 8,
    UpgradeTimelockNotExpired = 9,
    Unauthorized = 10,
    UpgradeAlreadyPending = 11,
    NoPendingUpgrade = 12,
    LimitTooHigh = 13,
    FeeTooHigh = 14,
    InvalidFeeRecipient = 15,
    FeeDisabled = 16,
    /// The same signer was presented more than once to a multisig call (issue #364).
    DuplicateSigner = 17,
    /// The pair could not be frozen/unfrozen: the cross-contract `set_frozen`
    /// call on the pair failed (pair not initialized, or an older pair WASM
    /// that does not implement the relay yet).
    PairFreezeFailed = 18,
    /// `freeze_pair` / `unfreeze_pair` targeted an address this factory did
    /// not create.
    PairNotFound = 19,
    /// The requested proposal_id does not exist in storage.
    ProposalNotFound = 20,
    /// `queue_proposal` was called on a proposal that has already been queued.
    ProposalAlreadyQueued = 21,
    /// `execute_proposal` was called before the timelock delay has elapsed.
    ProposalTimelockNotExpired = 22,
    /// `execute_proposal` was called on a proposal that has already been executed.
    ProposalAlreadyExecuted = 23,
    /// The proposal is not in the `Succeeded` state and therefore cannot be queued.
    ProposalNotSucceeded = 24,
    /// The cross-contract execution of a queued proposal's payload failed.
    ProposalExecutionFailed = 25,
}
