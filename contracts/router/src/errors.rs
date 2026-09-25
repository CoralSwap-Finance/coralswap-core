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
    InternalError = 309,
    CommitNotFound = 310,
    CommitRevealTooEarly = 311,
    CommitHashMismatch = 312,
    NonceAlreadyUsed = 313,
    /// Dust amount below minimum-reserve policy floor (issue 393).
    DustAmount = 314,
    /// Commit expired past its reveal window (issue 389).
    CommitExpired = 315,
    /// Too many live commits (issue 389).
    TooManyCommits = 316,
    /// Invalid commit-config values (issue 389).
    InvalidCommitConfig = 317,
}
