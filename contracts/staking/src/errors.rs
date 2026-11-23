use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum StakingError {
    /// `initialize` was called on an already-initialized contract.
    AlreadyInitialized = 300,
    /// A method that requires initialization was called before `initialize`.
    NotInitialized = 301,
    /// The caller is not the admin.
    Unauthorized = 302,
    /// A zero or negative amount was supplied where a positive value is required.
    InvalidAmount = 303,
    /// The user tried to unstake more than their current stake.
    InsufficientStake = 304,
    /// The supplied reward rate exceeds the hard cap.
    RewardRateTooHigh = 305,
}
