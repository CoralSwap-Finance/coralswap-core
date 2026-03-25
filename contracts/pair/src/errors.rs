use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum PairError {
    AlreadyInitialized = 100,
    NotInitialized = 101,
    InsufficientLiquidity = 102,
    InsufficientInputAmount = 103,
    InsufficientOutputAmount = 104,
    InvalidK = 105,
    Locked = 106,
    FlashLoanNotRepaid = 107,
    FlashPayloadTooLarge = 108,
    Paused = 109,
    Overflow = 110,
    ZeroAddress = 111,
    InsufficientLiquidityMinted = 112,
    InsufficientLiquidityBurned = 113,
    InvalidInput = 114,
    InvalidEmaAlpha = 115,
}
