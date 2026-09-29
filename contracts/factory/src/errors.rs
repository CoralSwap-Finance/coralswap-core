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
    /// The pair could not be frozen/unfrozen: the cross-contract `set_frozen`
    /// call on the pair failed (pair not initialized, or an older pair WASM
    /// that does not implement the relay yet).
    PairFreezeFailed = 17,
}
