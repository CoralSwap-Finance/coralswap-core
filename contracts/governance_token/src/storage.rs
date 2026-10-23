use soroban_sdk::{contracttype, Address, String};

#[contracttype]
pub struct TokenMetadata {
    pub decimals: u32,
    pub name: String,
    pub symbol: String,
}

#[contracttype]
pub struct AllowanceEntry {
    pub amount: i128,
    pub expiration_ledger: u32,
}

/// A voting-power (or total-supply) checkpoint keyed by ledger sequence.
///
/// Checkpoints are stored in ascending `ledger` order within a `Vec`.  `value`
/// holds either an address's voting power or the total supply, depending on
/// which storage key the checkpoint list lives under.
#[contracttype]
pub struct Checkpoint {
    pub ledger: u32,
    pub value: i128,
}

#[contracttype]
pub enum GovTokenKey {
    Balance(Address),
    Allowance(Address, Address),
    /// Who an address has delegated their voting power to.
    Delegate(Address),
    /// Sorted checkpoint list for an address's voting power.
    Checkpoints(Address),
    /// Sorted checkpoint list for the total supply.
    TotalSupplyCheckpoints,
    TotalSupply,
    Metadata,
    Admin,
}
