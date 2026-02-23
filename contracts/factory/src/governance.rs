use crate::errors::FactoryError;
use soroban_sdk::{Address, Env, Vec};

/// 2-of-3 multi-sig verification.
#[allow(dead_code)]
pub fn verify_multisig(
    _env: &Env,
    _signers: &Vec<Address>,
    _required: u32,
) -> Result<(), FactoryError> {
    todo!()
}
