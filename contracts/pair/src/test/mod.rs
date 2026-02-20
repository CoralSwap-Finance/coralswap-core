#![cfg(test)]

// ---------------------------------------------------------------------------
// Flash-loan test suite
//
// Structure
// ─────────
// 1. fee_tests   — pure unit tests for compute_flash_fee (no Env needed)
// 2. preflight   — pre-state validation (payload size, amounts); these hit
//                  guard checks before any storage access, so no pair state
//                  setup is required.
// 3. integration — full end-to-end flash loan tests using Soroban testutils:
//                  GoodReceiver (repays) and BadReceiver (does not repay).
// ---------------------------------------------------------------------------

mod swap_math;

// ============================================================================
// 1. Fee calculation unit tests
// ============================================================================
mod fee_tests {
    use crate::flash_loan::compute_flash_fee;

    #[test]
    fn floor_applied_when_current_fee_is_zero() {
        // floor = 5 bps → 1_000_000 * 5 / 10_000 = 500
        assert_eq!(compute_flash_fee(1_000_000, 0), 500);
    }

    #[test]
    fn dynamic_fee_used_when_higher_than_floor() {
        // 30 bps > 5 bps → 1_000_000 * 30 / 10_000 = 3_000
        assert_eq!(compute_flash_fee(1_000_000, 30), 3_000);
    }

    #[test]
    fn minimum_fee_is_one_stroop() {
        // 100 * 5 / 10_000 = 0 → clamped to 1
        assert_eq!(compute_flash_fee(100, 0), 1);
    }

    #[test]
    fn fee_exact_at_floor() {
        // 10_000 * 5 / 10_000 = 5
        assert_eq!(compute_flash_fee(10_000, 0), 5);
    }
    0
}

    #[test]
    fn fee_uses_max_of_current_and_floor() {
        // current_fee_bps = 3 < floor 5 → uses 5
        assert_eq!(compute_flash_fee(1_000_000, 3), 500);
    }
}

// ============================================================================
// 2. Pre-flight validation tests (no pair state needed)
// ============================================================================
mod preflight {
    use soroban_sdk::{testutils::Address as _, Address, Bytes, Env};

    use crate::{errors::PairError, flash_loan::execute_flash_loan};

    #[test]
    fn payload_exceeding_max_size_reverts() {
        let env = Env::default();
        // MAX_PAYLOAD_SIZE = 256; send 257 bytes
        let oversized = Bytes::from_slice(&env, &[0u8; 257]);
        let receiver = Address::generate(&env);

        let result = execute_flash_loan(&env, &receiver, 1_000, 0, &oversized);
        assert_eq!(result, Err(PairError::FlashPayloadTooLarge));
    }
    bals.push_back((id.clone(), amount));
    env.storage().instance().set(&symbol_short!("bals"), &bals);
}

    #[test]
    fn both_zero_amounts_reverts() {
        let env = Env::default();
        let data = Bytes::new(&env);
        let receiver = Address::generate(&env);

        let result = execute_flash_loan(&env, &receiver, 0, 0, &data);
        assert_eq!(result, Err(PairError::InsufficientInputAmount));
    }

    #[test]
    fn negative_amount_a_reverts() {
        let env = Env::default();
        let data = Bytes::new(&env);
        let receiver = Address::generate(&env);

        let result = execute_flash_loan(&env, &receiver, -1, 0, &data);
        assert_eq!(result, Err(PairError::InsufficientInputAmount));
    }

    #[test]
    fn negative_amount_b_reverts() {
        let env = Env::default();
        let data = Bytes::new(&env);
        let receiver = Address::generate(&env);

        let result = execute_flash_loan(&env, &receiver, 0, -1, &data);
        assert_eq!(result, Err(PairError::InsufficientInputAmount));
    }
}

// ============================================================================
// 3. Integration tests — full flash loan lifecycle
// ============================================================================
mod integration {
    use soroban_sdk::{
        contract, contractimpl,
        testutils::Address as _,
        token::{StellarAssetClient, TokenClient},
        Address, Bytes, Env,
    };

    use crate::{
        flash_loan::compute_flash_fee,
        storage::{DataKey, PairStorage},
        Pair, PairClient,
    };

    // -----------------------------------------------------------------------
    // Mock receiver that repays principal + fee for both borrowed tokens.
    // Isolated in its own submodule to avoid symbol collisions with BadReceiver.
    // -----------------------------------------------------------------------
    mod good_receiver_mod {
        use soroban_sdk::{contract, contractimpl, token::TokenClient, Address, Bytes, Env};

        #[contract]
        pub struct GoodReceiver;

        #[contractimpl]
        impl GoodReceiver {
            /// Repays principal + fee back to `initiator` (the pair contract).
            pub fn on_flash_loan(
                env: Env,
                initiator: Address, // pair contract — repayment destination
                token_a: Address,
                token_b: Address,
                amount_a: i128,
                amount_b: i128,
                fee_a: i128,
                fee_b: i128,
                _data: Bytes,
            ) {
                let me = env.current_contract_address();
                if amount_a > 0 {
                    TokenClient::new(&env, &token_a)
                        .transfer(&me, &initiator, &(amount_a + fee_a));
                }
                if amount_b > 0 {
                    TokenClient::new(&env, &token_b)
                        .transfer(&me, &initiator, &(amount_b + fee_b));
                }
            }
        }
    }
    use good_receiver_mod::GoodReceiver;

    // -----------------------------------------------------------------------
    // Mock receiver that deliberately skips repayment.
    // Isolated in its own submodule to avoid symbol collisions with GoodReceiver.
    // -----------------------------------------------------------------------
    mod bad_receiver_mod {
        use soroban_sdk::{contract, contractimpl, Address, Bytes, Env};

        #[contract]
        pub struct BadReceiver;

        #[contractimpl]
        impl BadReceiver {
            /// Intentionally does nothing — does not repay the loan.
            pub fn on_flash_loan(
                _env: Env,
                _initiator: Address,
                _token_a: Address,
                _token_b: Address,
                _amount_a: i128,
                _amount_b: i128,
                _fee_a: i128,
                _fee_b: i128,
                _data: Bytes,
            ) {
            }
        }
    }
    use bad_receiver_mod::BadReceiver;

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    /// Registers a Stellar Asset Contract for testing and mints `amount`
    /// stroops to `recipient`.  Returns the token address.
    fn create_token(env: &Env, admin: &Address, recipient: &Address, amount: i128) -> Address {
        let token_id = env.register_stellar_asset_contract_v2(admin.clone()).address();
        StellarAssetClient::new(env, &token_id).mint(recipient, &amount);
        token_id
    }

    /// Directly writes `PairStorage` into the pair contract's instance storage,
    /// bypassing the `initialize` stub so flash-loan tests are self-contained.
    fn setup_pair_state(
        env: &Env,
        pair_id: &Address,
        token_a: &Address,
        token_b: &Address,
        reserve_a: i128,
        reserve_b: i128,
    ) {
        env.as_contract(pair_id, || {
            env.storage().instance().set(
                &DataKey::PairState,
                &PairStorage {
                    factory: Address::generate(env),
                    token_a: token_a.clone(),
                    token_b: token_b.clone(),
                    lp_token: Address::generate(env),
                    reserve_a,
                    reserve_b,
                    block_timestamp_last: 0,
                    price_a_cumulative: 0,
                    price_b_cumulative: 0,
                    k_last: reserve_a * reserve_b,
                },
            );
        });
    }

    /// Reads the current `PairStorage` from the pair contract's instance storage.
    fn read_pair_state(env: &Env, pair_id: &Address) -> PairStorage {
        env.as_contract(pair_id, || {
            env.storage()
                .instance()
                .get(&DataKey::PairState)
                .unwrap()
        })
    }

    // -----------------------------------------------------------------------
    // Acceptance criterion 1: Loan repaid with fee passes
    // -----------------------------------------------------------------------
    #[test]
    fn flash_loan_repaid_with_fee_passes() {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let pair_id = env.register_contract(None, Pair);
        let receiver_id = env.register_contract(None, GoodReceiver);

        let reserve_a = 10_000_000_i128;
        let reserve_b = 10_000_000_i128;
        let amount_a = 1_000_000_i128;
        let fee_a = compute_flash_fee(amount_a, 0); // floor = 5 bps → 500

        // Pair holds initial reserves; receiver gets fee tokens pre-minted
        // to simulate profit from an arbitrage / other operation.
        let token_a = create_token(&env, &admin, &pair_id, reserve_a);
        StellarAssetClient::new(&env, &token_a).mint(&receiver_id, &fee_a);
        let token_b = create_token(&env, &admin, &pair_id, reserve_b);

        setup_pair_state(&env, &pair_id, &token_a, &token_b, reserve_a, reserve_b);

        let client = PairClient::new(&env, &pair_id);
        let data = Bytes::new(&env);
        // Should succeed without panicking.
        client.flash_loan(&receiver_id, &amount_a, &0_i128, &data);

        // Acceptance: reserves updated correctly post-loan.
        // Pair sent amount_a out, receiver returned amount_a + fee_a back.
        // New reserve_a = reserve_a + fee_a.
        let state = read_pair_state(&env, &pair_id);
        assert_eq!(state.reserve_a, reserve_a + fee_a, "reserve_a should grow by fee");
        assert_eq!(state.reserve_b, reserve_b, "reserve_b unchanged");

        // k-invariant must have improved.
        assert!(state.k_last >= reserve_a * reserve_b, "k must not decrease");
    }

    // -----------------------------------------------------------------------
    // Acceptance criterion 1 (dual token): Both tokens borrowed and repaid.
    // -----------------------------------------------------------------------
    #[test]
    fn flash_loan_both_tokens_repaid_passes() {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let pair_id = env.register_contract(None, Pair);
        let receiver_id = env.register_contract(None, GoodReceiver);

        let reserve_a = 10_000_000_i128;
        let reserve_b = 8_000_000_i128;
        let amount_a = 500_000_i128;
        let amount_b = 400_000_i128;
        let fee_a = compute_flash_fee(amount_a, 0);
        let fee_b = compute_flash_fee(amount_b, 0);

        let token_a = create_token(&env, &admin, &pair_id, reserve_a);
        StellarAssetClient::new(&env, &token_a).mint(&receiver_id, &fee_a);
        let token_b = create_token(&env, &admin, &pair_id, reserve_b);
        StellarAssetClient::new(&env, &token_b).mint(&receiver_id, &fee_b);

        setup_pair_state(&env, &pair_id, &token_a, &token_b, reserve_a, reserve_b);

        let client = PairClient::new(&env, &pair_id);
        let data = Bytes::new(&env);
        client.flash_loan(&receiver_id, &amount_a, &amount_b, &data);

        let state = read_pair_state(&env, &pair_id);
        assert_eq!(state.reserve_a, reserve_a + fee_a);
        assert_eq!(state.reserve_b, reserve_b + fee_b);
    }

    // -----------------------------------------------------------------------
    // Acceptance criterion 2: Loan without repayment reverts.
    // -----------------------------------------------------------------------
    #[test]
    fn flash_loan_without_repayment_reverts() {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let pair_id = env.register_contract(None, Pair);
        let receiver_id = env.register_contract(None, BadReceiver);

        let reserve_a = 10_000_000_i128;
        let reserve_b = 10_000_000_i128;

        let token_a = create_token(&env, &admin, &pair_id, reserve_a);
        let token_b = create_token(&env, &admin, &pair_id, reserve_b);

        setup_pair_state(&env, &pair_id, &token_a, &token_b, reserve_a, reserve_b);

        let client = PairClient::new(&env, &pair_id);
        let data = Bytes::new(&env);
        let result = client.try_flash_loan(&receiver_id, &1_000_000_i128, &0_i128, &data);
        assert!(result.is_err(), "un-repaid loan must revert");
    }

    // -----------------------------------------------------------------------
    // Acceptance criterion 3: Payload > MAX_PAYLOAD_SIZE reverts.
    // -----------------------------------------------------------------------
    #[test]
    fn flash_loan_oversized_payload_reverts() {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let pair_id = env.register_contract(None, Pair);
        let receiver_id = env.register_contract(None, GoodReceiver);

        let reserve_a = 10_000_000_i128;
        let reserve_b = 10_000_000_i128;

        let token_a = create_token(&env, &admin, &pair_id, reserve_a);
        let token_b = create_token(&env, &admin, &pair_id, reserve_b);
        setup_pair_state(&env, &pair_id, &token_a, &token_b, reserve_a, reserve_b);

        // 257 bytes — one byte over the 256-byte cap.
        let oversized = Bytes::from_slice(&env, &[0u8; 257]);
        let client = PairClient::new(&env, &pair_id);
        let result = client.try_flash_loan(&receiver_id, &1_000_000_i128, &0_i128, &oversized);
        assert!(result.is_err(), "oversized payload must revert");
    }

    // -----------------------------------------------------------------------
    // Guard: Requesting more than available reserves reverts.
    // -----------------------------------------------------------------------
    #[test]
    fn flash_loan_exceeding_reserves_reverts() {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let pair_id = env.register_contract(None, Pair);
        let receiver_id = env.register_contract(None, GoodReceiver);

        let reserve_a = 500_000_i128;
        let reserve_b = 10_000_000_i128;

        let token_a = create_token(&env, &admin, &pair_id, reserve_a);
        let token_b = create_token(&env, &admin, &pair_id, reserve_b);
        setup_pair_state(&env, &pair_id, &token_a, &token_b, reserve_a, reserve_b);

        let data = Bytes::new(&env);
        let client = PairClient::new(&env, &pair_id);
        // Request 1_000_000 but only 500_000 in reserves.
        let result = client.try_flash_loan(&receiver_id, &1_000_000_i128, &0_i128, &data);
        assert!(result.is_err(), "amount > reserve must revert");
    }

    // SEP-41 required stubs
    pub fn approve(_env: Env, _from: Address, _spender: Address, _amount: i128, _exp: u32) {}
    pub fn allowance(_env: Env, _from: Address, _spender: Address) -> i128 { 0 }
    pub fn transfer_from(_env: Env, _sp: Address, _from: Address, _to: Address, _amt: i128) {}
    pub fn burn(_env: Env, _from: Address, _amount: i128) {}
    pub fn burn_from(_env: Env, _sp: Address, _from: Address, _amount: i128) {}
    pub fn decimals(_env: Env) -> u32 { 7 }
    pub fn name(env: Env) -> soroban_sdk::String { soroban_sdk::String::from_str(&env, "Mock") }
    pub fn symbol(env: Env) -> soroban_sdk::String { soroban_sdk::String::from_str(&env, "MCK") }
}

// ── Setup helper ──────────────────────────────────────────────────────────────

fn make_pool(
    env: &Env,
    reserve_a: i128,
    reserve_b: i128,
) -> (Address, Address, Address) {
    let factory = Address::generate(env);
    let lp_token = Address::generate(env);

    let token_a = env.register_contract(None, MockToken);
    let token_b = env.register_contract(None, MockToken);
    let pair_addr = env.register_contract(None, Pair);

    PairClient::new(env, &pair_addr)
        .initialize(&factory, &token_a, &token_b, &lp_token);

    // Seed reserves: mint into pair, then sync reserves into storage.
    MockTokenClient::new(env, &token_a).mint(&pair_addr, &reserve_a);
    MockTokenClient::new(env, &token_b).mint(&pair_addr, &reserve_b);
    PairClient::new(env, &pair_addr).sync();

    (pair_addr, token_a, token_b)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[test]
fn test_swap_basic_b_out() {
    let env = Env::default();
    env.mock_all_auths();
    let (pair_addr, token_a, token_b) = make_pool(&env, 1_000_000, 1_000_000);
    let pair = PairClient::new(&env, &pair_addr);

    // Pre-deposit token_a into the pair (simulates the caller's transfer).
    // 115_000 A in comfortably covers the 30bps fee for 100_000 B out.
    MockTokenClient::new(&env, &token_a).mint(&pair_addr, &115_000);

    let to = Address::generate(&env);
    let result = pair.try_swap(&0, &100_000, &to);
    assert!(result.is_ok(), "basic swap should succeed: {result:?}");
    assert_eq!(
        MockTokenClient::new(&env, &token_b).balance(&to),
        100_000,
        "recipient should receive 100_000 token_b"
    );
}

#[test]
fn test_swap_reverts_zero_output() {
    let env = Env::default();
    env.mock_all_auths();
    let (pair_addr, _, _) = make_pool(&env, 1_000_000, 1_000_000);
    let pair = PairClient::new(&env, &pair_addr);

    let to = Address::generate(&env);
    let err = pair.try_swap(&0, &0, &to).unwrap_err().unwrap();
    assert_eq!(err, PairError::InsufficientOutputAmount);
}

#[test]
fn test_swap_reverts_invalid_k_no_input() {
    let env = Env::default();
    env.mock_all_auths();
    let (pair_addr, _, _) = make_pool(&env, 1_000_000, 1_000_000);
    let pair = PairClient::new(&env, &pair_addr);

    // No token_a pre-deposited → amount_in = 0 → K violated.
    let to = Address::generate(&env);
    let err = pair.try_swap(&0, &100_000, &to).unwrap_err().unwrap();
    assert!(
        err == PairError::InvalidK || err == PairError::InsufficientInputAmount,
        "expected InvalidK or InsufficientInputAmount, got {err:?}"
    );
}

#[test]
fn test_swap_reverts_output_exceeds_reserves() {
    let env = Env::default();
    env.mock_all_auths();
    let (pair_addr, token_a, _) = make_pool(&env, 1_000_000, 500_000);
    let pair = PairClient::new(&env, &pair_addr);

    MockTokenClient::new(&env, &token_a).mint(&pair_addr, &999_999_999);
    let to = Address::generate(&env);
    let err = pair
        .try_swap(&0, &600_000, &to)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, PairError::InsufficientLiquidity);
}

#[test]
fn test_swap_reentrancy_blocked() {
    let env = Env::default();
    env.mock_all_auths();
    let (pair_addr, _, _) = make_pool(&env, 1_000_000, 1_000_000);
    let pair = PairClient::new(&env, &pair_addr);

    // Manually lock the guard using the contract's context.
    env.as_contract(&pair_addr, || {
        set_reentrancy_guard(&env, &ReentrancyGuard { locked: true });
    });

    let to = Address::generate(&env);
    let err = pair.try_swap(&0, &1_000, &to).unwrap_err().unwrap();
    assert_eq!(err, PairError::Locked);

    // Reset guard.
    env.as_contract(&pair_addr, || {
        set_reentrancy_guard(&env, &ReentrancyGuard { locked: false });
    });
}

#[test]
fn test_swap_fee_sufficient_input_succeeds() {
    let env = Env::default();
    env.mock_all_auths();
    let (pair_addr, token_a, _) = make_pool(&env, 1_000_000, 1_000_000);
    let pair = PairClient::new(&env, &pair_addr);

    MockTokenClient::new(&env, &token_a).mint(&pair_addr, &10_500);
    let to = Address::generate(&env);
    let result = pair.try_swap(&0, &10_000, &to);
    assert!(
        result.is_ok(),
        "swap with sufficient input+fee should succeed: {result:?}"
    );
}

#[test]
fn test_get_reserves_reflects_swap() {
    let env = Env::default();
    env.mock_all_auths();
    let (pair_addr, token_a, _) = make_pool(&env, 1_000_000, 1_000_000);
    let pair = PairClient::new(&env, &pair_addr);

    let (ra, rb, _) = pair.get_reserves();
    assert_eq!(ra, 1_000_000);
    assert_eq!(rb, 1_000_000);

    MockTokenClient::new(&env, &token_a).mint(&pair_addr, &11_000);
    let to = Address::generate(&env);
    pair.swap(&0, &10_000, &to);

    let (ra2, rb2, _) = pair.get_reserves();
    assert!(ra2 > 1_000_000, "reserve_a should increase after swap");
    assert_eq!(rb2, 990_000, "reserve_b should equal 1_000_000 - 10_000");
}

#[test]
fn test_get_current_fee_bps_default() {
    let env = Env::default();
    env.mock_all_auths();
    let (pair_addr, _, _) = make_pool(&env, 1_000_000, 1_000_000);
    let pair = PairClient::new(&env, &pair_addr);

    // No trades → vol_accumulator = 0 → baseline 30 bps.
    assert_eq!(pair.get_current_fee_bps(), 30);
}
