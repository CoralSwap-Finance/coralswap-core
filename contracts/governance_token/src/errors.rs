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
}
