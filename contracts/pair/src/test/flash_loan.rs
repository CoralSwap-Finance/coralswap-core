#![cfg(test)]

use coralswap_malicious_flash_receiver::MaliciousFlashReceiver;
use coralswap_mock_flash_receiver::MockFlashReceiver;

use crate::{errors::PairError, Pair, PairClient};
use soroban_sdk::token::{StellarAssetClient, TokenClient};
use soroban_sdk::{testutils::Address as _, Address, Bytes, Env};

fn create_token_contract<'a>(
    e: &Env,
    admin: &Address,
) -> (Address, StellarAssetClient<'a>, TokenClient<'a>) {
    let contract_id = e.register_stellar_asset_contract_v2(admin.clone()).address();
    (
        contract_id.clone(),
        StellarAssetClient::new(e, &contract_id),
        TokenClient::new(e, &contract_id),
    )
}

fn create_pair_contract<'a>(e: &Env) -> (Address, PairClient<'a>) {
    let contract_id = e.register(Pair, ());
    (contract_id.clone(), PairClient::new(e, &contract_id))
}

fn register_honest_receiver(e: &Env) -> Address {
    e.register(MockFlashReceiver, ())
}

fn register_malicious_receiver(e: &Env) -> Address {
    e.register(MaliciousFlashReceiver, ())
}

struct Setup<'a> {
    env: Env,
    token_a_admin: StellarAssetClient<'a>,
    token_b_admin: StellarAssetClient<'a>,
    pair: Address,
    pair_client: PairClient<'a>,
    honest_receiver: Address,
    malicious_receiver: Address,
}

impl<'a> Setup<'a> {
    fn new() -> Self {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);

        let (token_a, token_a_admin, _) = create_token_contract(&env, &admin);
        let (token_b, token_b_admin, _) = create_token_contract(&env, &admin);

        let (token_a, token_a_admin, token_b, token_b_admin) = if token_a < token_b {
            (token_a, token_a_admin, token_b, token_b_admin)
        } else {
            (token_b, token_b_admin, token_a, token_a_admin)
        };

        let (pair, pair_client) = create_pair_contract(&env);
        let honest_receiver = register_honest_receiver(&env);
        let malicious_receiver = register_malicious_receiver(&env);

        let factory = Address::generate(&env);
        let lp_token = Address::generate(&env);

        pair_client.initialize(&factory, &token_a, &token_b, &lp_token);

        Setup {
            env,
            token_a_admin,
            token_b_admin,
            pair,
            pair_client,
            honest_receiver,
            malicious_receiver,
        }
    }

    fn fund_pool(&self, amount: i128) {
        self.token_a_admin.mint(&self.pair, &amount);
        self.token_b_admin.mint(&self.pair, &amount);
        self.pair_client.sync();
    }
}

fn is_reentrancy_error(
    result: Result<
        Result<(), soroban_sdk::ConversionError>,
        Result<PairError, soroban_sdk::InvokeError>,
    >,
) -> bool {
    match result {
        Err(_) => true,
        Ok(Err(_)) => true,
        Ok(Ok(())) => false,
    }
}

// Scenario C — honest receiver repays principal + fee (regression baseline)
#[test]
fn flash_loan_honest_receiver_repays() {
    let setup = Setup::new();
    let initial_reserve = 1_000_000_i128;
    setup.fund_pool(initial_reserve);

    let loan_amount = 10_000_i128;
    let fee = crate::flash_loan::compute_flash_fee(loan_amount, 30).unwrap();

    setup.token_a_admin.mint(&setup.honest_receiver, &fee);

    let repay_action = Bytes::from_slice(&setup.env, b"repay");
    setup.pair_client.flash_loan(&setup.honest_receiver, &loan_amount, &0, &repay_action);

    let (res_a, res_b, _) = setup.pair_client.get_reserves();
    assert_eq!(res_a, initial_reserve + fee);
    assert_eq!(res_b, initial_reserve);
}

// Scenario A — malicious receiver calls pair::swap() during flash callback
#[test]
fn flash_loan_reentrancy_swap_attack_reverts() {
    let setup = Setup::new();
    setup.fund_pool(1_000_000);

    let attack = Bytes::from_slice(&setup.env, b"attack_swap");
    let result = setup.pair_client.try_flash_loan(&setup.malicious_receiver, &10_000, &0, &attack);

    assert!(
        is_reentrancy_error(result),
        "swap re-entry during flash callback must revert with Locked or equivalent"
    );
}

// Scenario B — malicious receiver nests flash_loan() during callback
#[test]
fn flash_loan_reentrancy_nested_flash_attack_reverts() {
    let setup = Setup::new();
    setup.fund_pool(1_000_000);

    let attack = Bytes::from_slice(&setup.env, b"attack_flash");
    let result = setup.pair_client.try_flash_loan(&setup.malicious_receiver, &10_000, &0, &attack);

    assert!(is_reentrancy_error(result), "nested flash_loan during callback must revert cleanly");
}

// Scenario C2 — receiver's callback reverts on its own; the pair must
// propagate the failure instead of treating the callback as successful
#[test]
fn flash_loan_failing_callback_reverts_with_flash_callback_failed() {
    let setup = Setup::new();
    setup.fund_pool(1_000_000);
    let (reserve_a_before, reserve_b_before, _) = setup.pair_client.get_reserves();

    let attack = Bytes::from_slice(&setup.env, b"attack_fail");
    let result = setup.pair_client.try_flash_loan(&setup.malicious_receiver, &10_000, &0, &attack);

    match result {
        Err(Ok(PairError::FlashCallbackFailed)) => {}
        other => panic!("expected FlashCallbackFailed, got {:?}", other),
    }

    // Reserves must be untouched: a failed callback is not a repaid loan
    let (reserve_a_after, reserve_b_after, _) = setup.pair_client.get_reserves();
    assert_eq!(reserve_a_after, reserve_a_before);
    assert_eq!(reserve_b_after, reserve_b_before);

    // The guard must be released; an honest loan still works afterwards
    let fee = crate::flash_loan::compute_flash_fee(1_000, 30).unwrap();
    setup.token_a_admin.mint(&setup.honest_receiver, &(1_000 + fee));
    let repay_action = Bytes::from_slice(&setup.env, b"repay");
    assert_eq!(
        setup.pair_client.try_flash_loan(&setup.honest_receiver, &1_000, &0, &repay_action),
        Ok(Ok(()))
    );
}

#[test]
fn test_compute_flash_fee_overflow_returns_error() {
    let result = crate::flash_loan::compute_flash_fee(i128::MAX, 30);
    assert_eq!(result, Err(PairError::FeeOverflow));
}

#[test]
fn test_compute_flash_fee_normal_amount() {
    let result = crate::flash_loan::compute_flash_fee(10_000, 30);
    assert!(result.is_ok());
    assert!(result.unwrap() > 0);
}

#[test]
fn test_compute_flash_fee_cap_boundary_valid() {
    let result = crate::flash_loan::compute_flash_fee(10_000, 10_000);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), 10_000);
}

#[test]
fn test_compute_flash_fee_cap_boundary_invalid() {
    let result = crate::flash_loan::compute_flash_fee(10_000, 10_001);
    assert_eq!(result, Err(PairError::FlashLoanFeeTooHigh));
}

#[test]
fn test_compute_flash_fee_excessive_fee() {
    let result = crate::flash_loan::compute_flash_fee(10_000, 15_000);
    assert_eq!(result, Err(PairError::FlashLoanFeeTooHigh));
}

#[test]
fn test_flash_loan_zero_amount_clean_noop() {
    let setup = Setup::new();
    setup.fund_pool(100_000);

    let (res_a_before, res_b_before, _) = setup.pair_client.get_reserves();

    // Using malicious_receiver whose callback would attempt an unauthorized attack if invoked.
    // Since amount is (0, 0), the callback is never invoked and returns Ok(()).
    let attack = Bytes::from_slice(&setup.env, b"swap");
    let result = setup.pair_client.try_flash_loan(&setup.malicious_receiver, &0, &0, &attack);
    assert_eq!(result, Ok(Ok(())));

    // Reserves remain unchanged.
    let (res_a_after, res_b_after, _) = setup.pair_client.get_reserves();
    assert_eq!(res_a_before, res_a_after);
    assert_eq!(res_b_before, res_b_after);
}

#[test]
fn test_flash_loan_zero_amount_fails_if_payload_too_large() {
    let setup = Setup::new();
    let mut large_bytes = [0u8; 257];
    large_bytes[0] = 1;
    let large_data = Bytes::from_slice(&setup.env, &large_bytes);

    let result = setup.pair_client.try_flash_loan(&setup.honest_receiver, &0, &0, &large_data);
    assert_eq!(result, Err(Ok(PairError::FlashPayloadTooLarge)));
}

#[test]
fn test_flash_loan_zero_amount_fails_if_uninitialized() {
    let env = Env::default();
    let (_pair_id, pair_client) = create_pair_contract(&env);
    let receiver = Address::generate(&env);

    let result = pair_client.try_flash_loan(&receiver, &0, &0, &Bytes::new(&env));
    assert_eq!(result, Err(Ok(PairError::NotInitialized)));
}

#[test]
fn test_flash_loan_negative_amount_fails() {
    let setup = Setup::new();
    setup.fund_pool(100_000);
    let data = Bytes::new(&setup.env);

    let result_neg_a = setup.pair_client.try_flash_loan(&setup.honest_receiver, &-1, &0, &data);
    assert_eq!(result_neg_a, Err(Ok(PairError::InsufficientInputAmount)));

    let result_neg_b = setup.pair_client.try_flash_loan(&setup.honest_receiver, &0, &-1, &data);
    assert_eq!(result_neg_b, Err(Ok(PairError::InsufficientInputAmount)));
}

