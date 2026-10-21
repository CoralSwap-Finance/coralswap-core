use soroban_sdk::contracterror;

/// Error surface of the concentrated-liquidity pair.
///
/// Codes live in the 400 range so they can never be conflated with
/// `PairError` (100s) or `OracleError` / `LpTokenError` (200s) at a call
/// site that mixes clients.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum ConcentratedPairError {
    AlreadyInitialized = 400,
    NotInitialized = 401,
    /// Malformed constructor-style input (identical or self-referential
    /// tokens, zero/oversized fee or tick spacing).
    InvalidInput = 402,
    /// `sqrt_price` outside `[MIN_SQRT_RATIO, MAX_SQRT_RATIO)`.
    InvalidSqrtPrice = 403,
    /// Tick outside `[MIN_TICK, MAX_TICK]`.
    InvalidTick = 404,
    Overflow = 405,
}
