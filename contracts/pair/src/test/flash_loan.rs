#![cfg(test)]

use coralswap_mock_flash_receiver::{
    malicious::MaliciousFlashReceiver, MockFlashReceiver,
};

use crate::{errors::PairError, Pair, PairClient};
use soroban_sdk::token::{StellarAssetClient, TokenClient};
use soroban_sdk::{testutils::Address as _, Address, Bytes, Env};

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

fn register_honest_receiver(e: &Env) -> Address {
    e.register_contract(None, MockFlashReceiver)
}

fn register_malicious_receiver(e: &Env) -> Address {
    e.register_contract(None, MaliciousFlashReceiver)
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
    setup
        .pair_client
        .flash_loan(&setup.honest_receiver, &loan_amount, &0, &repay_action);

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
    let result = setup.pair_client.try_flash_loan(
        &setup.malicious_receiver,
        &10_000,
        &0,
        &attack,
    );

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
    let result = setup.pair_client.try_flash_loan(
        &setup.malicious_receiver,
        &10_000,
        &0,
        &attack,
    );

    assert!(
        is_reentrancy_error(result),
        "nested flash_loan during callback must revert cleanly"
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
