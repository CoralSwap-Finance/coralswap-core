use crate::errors::FactoryError;
use soroban_sdk::{BytesN, Env};

/// Proposed a timelocked contract upgrade (72h delay).
#[allow(dead_code)]
pub fn propose_upgrade(_env: &Env, _new_wasm_hash: BytesN<32>) -> Result<(), FactoryError> {
    todo!()
}

/// Executed a previously proposed upgrade after timelock expiry.
#[allow(dead_code)]
pub fn execute_upgrade(_env: &Env) -> Result<(), FactoryError> {
    todo!()
}
