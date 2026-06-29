#![cfg(test)]

use crate::{LpToken, LpTokenClient};
use crate::storage::LpTokenKey;
use crate::errors::LpTokenError;
use soroban_sdk::{
    testutils::Address as _,
    Address,
    Bytes,
    BytesN,
    Env,
    Ledger,
    String,
    xdr::ToXdr,
};
use ed25519_dalek::{Keypair, PublicKey, SecretKey, Signer};

#[test]
fn test_contract_compiles() {
    // This test ensures the contract compiles successfully
    assert!(true);
}

#[test]
fn test_approve_rejects_current_ledger_expiration() {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();
    let contract_id = env.register_contract(None, LpToken);
    let client = LpTokenClient::new(&env, &contract_id);
    let owner = Address::generate(&env);
    let spender = Address::generate(&env);
    let current_ledger = env.ledger().sequence();

    let result = client.try_approve(&owner, &spender, &100_i128, &current_ledger);

    assert_eq!(result, Err(Ok(LpTokenError::InvalidExpiration)));
    assert_eq!(client.allowance(&owner, &spender), 0);
}

#[test]
fn test_approve_allows_future_expiration_and_transfer_from_deducts_allowance() {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();
    let contract_id = env.register_contract(None, LpToken);
    let client = LpTokenClient::new(&env, &contract_id);
    let owner = Address::generate(&env);
    let spender = Address::generate(&env);
    let receiver = Address::generate(&env);
    let current_ledger = env.ledger().sequence();

    client.approve(&owner, &spender, &100_i128, &(current_ledger + 1));
    assert_eq!(client.allowance(&owner, &spender), 100);

    env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .set(&LpTokenKey::Balance(owner.clone()), &100_i128);
    });

    client.transfer_from(&spender, &owner, &receiver, &25_i128).unwrap();

    assert_eq!(client.allowance(&owner, &spender), 75);
    assert_eq!(client.balance(&receiver), 25);
    assert_eq!(client.balance(&owner), 75);
}

// ── Permit (SEP-41) Tests ────────────────────────────────────────────────────

fn make_keypair() -> Keypair {
    let mut seed = [0u8; 64];
    seed[..32].copy_from_slice(&[1u8; 32]);
    let secret = SecretKey::from_bytes(&seed[..32]).unwrap();
    let public = PublicKey::from(&secret);
    seed[32..].copy_from_slice(&public.to_bytes());
    Keypair::from_bytes(&seed).unwrap()
}

fn owner_address(env: &Env, keypair: &Keypair) -> Address {
    let pk = BytesN::from_array(env, &keypair.public.to_bytes());
    Address::Account(pk)
}

fn permit_digest(env: &Env, owner: &Address, spender: &Address, amount: i128, nonce: u64, deadline: u32) -> BytesN<32> {
    let mut data = Bytes::new(env);
    data.append(&owner.clone().to_xdr(env));
    data.append(&spender.clone().to_xdr(env));
    data.append(&amount.to_be_bytes());
    data.append(&nonce.to_be_bytes());
    data.append(&deadline.to_be_bytes());
    env.crypto().sha256(&data)
}

fn sign_permit(env: &Env, keypair: &Keypair, owner: &Address, spender: &Address, amount: i128, nonce: u64, deadline: u32) -> BytesN<64> {
    let digest = permit_digest(env, owner, spender, amount, nonce, deadline);
    let signature = keypair.sign(digest.as_ref());
    BytesN::from_array(env, &signature.to_bytes())
}

#[test]
fn test_permit_approves_spender_without_on_chain_approval() {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();
    let contract_id = env.register_contract(None, LpToken);
    let client = LpTokenClient::new(&env, &contract_id);
    
    let owner_keys = make_keypair();
    let owner = owner_address(&env, &owner_keys);
    let spender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let amount = 1_000_i128;
    let deadline = env.ledger().sequence() + 10;

    // Mint tokens to owner
    env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .set(&LpTokenKey::Balance(owner.clone()), &amount);
    });

    // Check initial nonce is 0
    let nonce = client.nonce(&owner);
    assert_eq!(nonce, 0);

    // Sign permit
    let signature = sign_permit(&env, &owner_keys, &owner, &spender, amount, nonce, deadline);

    // Execute permit
    let permit_result = client.try_permit(&owner, &spender, &amount, &deadline, &signature);
    assert!(permit_result.is_ok());

    // Verify allowance was set
    assert_eq!(client.allowance(&owner, &spender), amount);

    // Verify transfer_from works with permit-set allowance
    client.transfer_from(&spender, &owner, &recipient, &amount).unwrap();
    assert_eq!(client.balance(&recipient), amount);
}

#[test]
fn test_permit_expired_deadline_reverts() {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();
    let contract_id = env.register_contract(None, LpToken);
    let client = LpTokenClient::new(&env, &contract_id);
    
    let owner_keys = make_keypair();
    let owner = owner_address(&env, &owner_keys);
    let spender = Address::generate(&env);
    let amount = 1_000_i128;
    let deadline = env.ledger().sequence().saturating_sub(1);
    let nonce = 0;

    let signature = sign_permit(&env, &owner_keys, &owner, &spender, amount, nonce, deadline);
    let result = client.try_permit(&owner, &spender, &amount, &deadline, &signature);

    assert_eq!(result, Err(Ok(LpTokenError::PermitExpired)));
}

#[test]
fn test_permit_replayed_nonce_reverts() {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();
    let contract_id = env.register_contract(None, LpToken);
    let client = LpTokenClient::new(&env, &contract_id);
    
    let owner_keys = make_keypair();
    let owner = owner_address(&env, &owner_keys);
    let spender = Address::generate(&env);
    let amount = 1_000_i128;
    let deadline = env.ledger().sequence() + 10;

    let nonce = client.nonce(&owner);
    let signature = sign_permit(&env, &owner_keys, &owner, &spender, amount, nonce, deadline);

    // First permit should succeed
    let first_result = client.try_permit(&owner, &spender, &amount, &deadline, &signature);
    assert!(first_result.is_ok());

    // Nonce should be incremented
    let new_nonce = client.nonce(&owner);
    assert_eq!(new_nonce, nonce + 1);

    // Replaying the same signature should fail
    let replay_result = client.try_permit(&owner, &spender, &amount, &deadline, &signature);
    assert_eq!(replay_result, Err(Ok(LpTokenError::InvalidSignature)));
}

#[test]
fn test_permit_invalid_signature_reverts() {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();
    let contract_id = env.register_contract(None, LpToken);
    let client = LpTokenClient::new(&env, &contract_id);
    
    let owner_keys = make_keypair();
    let owner = owner_address(&env, &owner_keys);
    let spender = Address::generate(&env);
    let amount = 1_000_i128;
    let deadline = env.ledger().sequence() + 10;
    let nonce = 0;

    // Sign for a different amount
    let bad_signature = sign_permit(&env, &owner_keys, &owner, &spender, amount + 1, nonce, deadline);

    // Try to use bad signature with original amount
    let result = client.try_permit(&owner, &spender, &amount, &deadline, &bad_signature);
    assert_eq!(result, Err(Ok(LpTokenError::InvalidSignature)));
}
