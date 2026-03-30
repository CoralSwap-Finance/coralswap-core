#![cfg(test)]

use crate::{errors::PairError, Pair, PairClient};
use soroban_sdk::token::{StellarAssetClient, TokenClient};
use soroban_sdk::{testutils::Address as _, Address, Bytes, Env};

// We will test `on_flash_loan` using our newly minted mockup
// Note: We need to register the mock flash receiver contract in the test environment

mod mock_receiver {
    soroban_sdk::contractimport!(
        file = "../../target/wasm32-unknown-unknown/release/coralswap_mock_flash_receiver.wasm"
    );
}

fn create_token_contract<'a>(
    e: &Env,
    admin: &Address,
) -> (Address, StellarAssetClient<'a>, TokenClient<'a>) {
    let contract_id = e.register_stellar_asset_contract(admin.clone());
    (
        contract_id.clone(),
        StellarAssetClient::new(e, &contract_id),
        TokenClient::new(e, &contract_id),
    )
}

fn create_pair_contract<'a>(e: &Env) -> (Address, PairClient<'a>) {
    let contract_id = e.register_contract(None, Pair);
    (contract_id.clone(), PairClient::new(e, &contract_id))
}

fn create_mock_receiver(e: &Env) -> Address {
    // Register the mock using the WASM
    e.register_contract_wasm(None, mock_receiver::WASM)
}

struct Setup<'a> {
    env: Env,
    admin: Address,
    user: Address,
    token_a: Address,
    token_a_admin: StellarAssetClient<'a>,
    token_a_client: TokenClient<'a>,
    token_b: Address,
    token_b_admin: StellarAssetClient<'a>,
    token_b_client: TokenClient<'a>,
    pair: Address,
    pair_client: PairClient<'a>,
    receiver: Address,
}

impl<'a> Setup<'a> {
    fn new() -> Self {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let user = Address::generate(&env);

        let (token_a, token_a_admin, token_a_client) = create_token_contract(&env, &admin);
        let (token_b, token_b_admin, token_b_client) = create_token_contract(&env, &admin);

        // Ensure token_a < token_b lexicographically for standard setup
        let (token_a, token_a_admin, token_a_client, token_b, token_b_admin, token_b_client) =
            if token_a < token_b {
                (token_a, token_a_admin, token_a_client, token_b, token_b_admin, token_b_client)
            } else {
                (token_b, token_b_admin, token_b_client, token_a, token_a_admin, token_a_client)
            };

        let (pair, pair_client) = create_pair_contract(&env);
        let receiver = create_mock_receiver(&env);

        let factory = Address::generate(&env);
        let lp_token = Address::generate(&env); // Fake LP for now, maybe we need a real one

        pair_client.initialize(&factory, &token_a, &token_b, &lp_token);

        Setup {
            env,
            admin,
            user,
            token_a,
            token_a_admin,
            token_a_client,
            token_b,
            token_b_admin,
            token_b_client,
            pair,
            pair_client,
            receiver,
        }
    }
}

// ---------------------------------------------------------------------------
// Integration tests - temporarily disabled until flash_loan is exposed
// ---------------------------------------------------------------------------

/*
#[test]
fn test_flash_loan_repay() {
    let setup = Setup::new();

    // Add liquidity to pair
    let initial_reserve = 1_000_000;
    setup.token_a_admin.mint(&setup.pair, &initial_reserve);
    setup.token_b_admin.mint(&setup.pair, &initial_reserve);
    setup.pair_client.sync();

    let loan_amount = 10_000;
    // compute_flash_fee now returns Result — unwrap is safe for a normal amount.
    let fee = crate::flash_loan::compute_flash_fee(loan_amount, 30).unwrap();

    // Fund the receiver with enough tokens to pay the fee!
    setup.token_a_admin.mint(&setup.receiver, &fee);
    setup.token_b_admin.mint(&setup.receiver, &(fee as i128)); // Cast just for simplicity

    let repay_action = Bytes::from_slice(&setup.env, b"repay");

    setup.pair_client.flash_loan(
        &setup.receiver,
        &loan_amount,
        &0,
        &repay_action,
    );

    // Check invariants... reserves should have increased by fee
    let (res_a, res_b, _) = setup.pair_client.get_reserves();
    assert_eq!(res_a, initial_reserve + fee);
    assert_eq!(res_b, initial_reserve);
}

#[test]
#[should_panic(expected = "HostError: Error(Value, InvalidInput)")]
fn test_flash_loan_steal() {
    let setup = Setup::new();

    let initial_reserve = 1_000_000;
    setup.token_a_admin.mint(&setup.pair, &initial_reserve);
    setup.token_b_admin.mint(&setup.pair, &initial_reserve);
    setup.pair_client.sync();

    let steal_action = Bytes::from_slice(&setup.env, b"steal");

    // This should panic due to FlashLoanNotRepaid
    setup.pair_client.flash_loan(
        &setup.receiver,
        &10_000,
        &0,
        &steal_action,
    );
}
*/

// ---------------------------------------------------------------------------
// Unit test: fee overflow returns FeeOverflow error, not i128::MAX
// ---------------------------------------------------------------------------

#[test]
fn test_compute_flash_fee_overflow_returns_error() {
    // i128::MAX * any positive bps overflows — should return FeeOverflow.
    let result = crate::flash_loan::compute_flash_fee(i128::MAX, 30);
    assert_eq!(result, Err(PairError::FeeOverflow), "overflow must return FeeOverflow");
}

#[test]
fn test_compute_flash_fee_normal_amount() {
    // Normal amount should succeed and return a positive fee.
    let result = crate::flash_loan::compute_flash_fee(10_000, 30);
    assert!(result.is_ok(), "normal amount must succeed");
    assert!(result.unwrap() > 0, "fee must be positive");
}

// ---------------------------------------------------------------------------
// Unit tests: fee_bps validation (cap at 10_000)
// ---------------------------------------------------------------------------

#[test]
fn test_compute_flash_fee_cap_boundary_valid() {
    // fee_bps == 10_000 (100%) is allowed as edge case
    let result = crate::flash_loan::compute_flash_fee(10_000, 10_000);
    assert!(result.is_ok(), "fee_bps == 10_000 must be allowed");
    assert_eq!(result.unwrap(), 10_000, "100% fee should equal principal");
}

#[test]
fn test_compute_flash_fee_cap_boundary_invalid() {
    // fee_bps > 10_000 returns FlashLoanFeeTooHigh error
    let result = crate::flash_loan::compute_flash_fee(10_000, 10_001);
    assert_eq!(result, Err(PairError::FlashLoanFeeTooHigh), "fee_bps > 10_000 must return FlashLoanFeeTooHigh");
}

#[test]
fn test_compute_flash_fee_excessive_fee() {
    // fee_bps = 15000 (150%) should be rejected
    let result = crate::flash_loan::compute_flash_fee(10_000, 15_000);
    assert_eq!(result, Err(PairError::FlashLoanFeeTooHigh), "fee_bps = 15000 must return FlashLoanFeeTooHigh");
}
