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
}
