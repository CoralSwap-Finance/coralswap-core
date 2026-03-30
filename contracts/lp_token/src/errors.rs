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
    ContractPaused = 206,
}
