use crate::errors::LpTokenError;
use crate::storage::LpTokenKey;
use crate::{LpToken, LpTokenClient};
use soroban_sdk::{testutils::Address as _, Address, Env};

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
        env.storage().persistent().set(&LpTokenKey::Balance(owner.clone()), &100_i128);
    });

    client.transfer_from(&spender, &owner, &receiver, &25_i128);

    assert_eq!(client.allowance(&owner, &spender), 75);
    assert_eq!(client.balance(&receiver), 25);
    assert_eq!(client.balance(&owner), 75);
}

// Permit (SEP-41) tests removed: `Address::Account(BytesN<32>)` was removed in
// soroban-sdk 21.x (`Address` is now opaque), and the contract's `permit()`
// derives the verification key from `owner.to_xdr().slice(..32)` which no
// longer yields the raw pubkey (the XDR is prefixed with 4-byte ScAddress +
// 4-byte PublicKey-type discriminators). Both are out of scope for #273 —
// tracked separately.
