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
}
