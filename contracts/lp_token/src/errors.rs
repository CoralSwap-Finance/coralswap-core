use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum LpTokenError {
    AlreadyInitialized = 200,
    NotInitialized = 201,
    Unauthorized = 202,
    InsufficientBalance = 203,
    InsufficientAllowance = 204,
    Overflow = 205,
    InvalidExpiration = 206,
    ContractPaused = 207,
    PermitExpired = 208,
    InvalidSignature = 209,
    /// Metadata failed SAC-parity validation (issue 392).
    InvalidMetadata = 210,
    /// Mint amount exceeds per-call limit (issue #349).
    MintAmountTooLarge = 211,
    /// Total supply would exceed maximum allowed (issue #349).
    TotalSupplyExceeded = 212,
}
