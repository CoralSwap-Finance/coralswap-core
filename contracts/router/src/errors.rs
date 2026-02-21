use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum RouterError {
    Expired = 300,
    InsufficientOutputAmount = 301,
    ExcessiveInputAmount = 302,
    InvalidPath = 303,
    PairNotFound = 304,
    IdenticalTokens = 305,
    ZeroAmount = 306,
    InsufficientLiquidity = 307,
    SlippageExceeded = 308,
}
