use soroban_sdk::Env;

// Compiled WASM bytecode for cross-contract deployment in tests.
// Must build with: cargo build --target wasm32v1-none --release
const PAIR_WASM: &[u8] =
    include_bytes!("../../../../target/wasm32v1-none/release/coralswap_pair.wasm");
const LP_TOKEN_WASM: &[u8] =
    include_bytes!("../../../../target/wasm32v1-none/release/coralswap_lp_token.wasm");

mod factory_tests {
    use super::*;
    use crate::{Factory, FactoryClient};
    use soroban_sdk::{testutils::Address as _, Address, Bytes, Vec};

    /// Helper: sets up a fresh Env, deploys the factory, initializes it with
    /// real pair / LP-token WASM hashes, and returns commonly-needed handles.
    fn setup_env<'a>() -> (Env, FactoryClient<'a>, Address, Address, Address, Address) {
        let env = Env::default();
        env.mock_all_auths();

        let factory_address = env.register_contract(None, Factory);
        let client = FactoryClient::new(&env, &factory_address);

        let signer_1 = Address::generate(&env);
        let signer_2 = Address::generate(&env);
        let signer_3 = Address::generate(&env);
        let fee_to_setter = Address::generate(&env);

        // Upload real WASM so deployer().deploy() produces working contracts.
        let pair_wasm_hash =
            env.deployer().upload_contract_wasm(Bytes::from_slice(&env, PAIR_WASM));
        let lp_token_wasm_hash =
            env.deployer().upload_contract_wasm(Bytes::from_slice(&env, LP_TOKEN_WASM));

        client.initialize(
            &Vec::from_array(&env, [signer_1, signer_2, signer_3]),
            &pair_wasm_hash,
            &lp_token_wasm_hash,
            &fee_to_setter,
        );

        let token_a = Address::generate(&env);
        let token_b = Address::generate(&env);

        (env, client, token_a, token_b, factory_address, fee_to_setter)
    }

    // ---------- Happy path ----------

    #[test]
    fn test_initialize_happy_path() {
        let env = Env::default();
        let factory_address = env.register_contract(None, Factory);
        let client = FactoryClient::new(&env, &factory_address);
        let fee_to_setter = Address::generate(&env);

        let signer_1 = Address::generate(&env);
        let signer_2 = Address::generate(&env);
        let signer_3 = Address::generate(&env);

        let pair_wasm_hash = env.deployer().upload_contract_wasm(Bytes::new(&env));
        let lp_token_wasm_hash = env.deployer().upload_contract_wasm(Bytes::new(&env));

        // Should succeed
        client.initialize(
            &Vec::from_array(&env, [signer_1, signer_2, signer_3]),
            &pair_wasm_hash,
            &lp_token_wasm_hash,
            &fee_to_setter,
        );

        // Verify state after init
        assert!(!client.is_paused());
        assert!(client.fee_to().is_none());
        assert_eq!(client.fee_to_setter(), Some(fee_to_setter));
    }

    // ---------- Double-init guard ----------

    #[test]
    fn test_initialize_double_init_fails() {
        let (env, client, _, _, _, _) = setup_env();

        let signer = Address::generate(&env);
        let fee_to_setter = Address::generate(&env);
        let pair_wasm_hash = env.deployer().upload_contract_wasm(Bytes::new(&env));
        let lp_token_wasm_hash = env.deployer().upload_contract_wasm(Bytes::new(&env));

        // Second call should fail with AlreadyInitialized (error code 1)
        let result = client.try_initialize(
            &Vec::from_array(&env, [signer]),
            &pair_wasm_hash,
            &lp_token_wasm_hash,
            &fee_to_setter,
        );
        assert!(result.is_err());
    }

    // ---------- Signer validation ----------

    #[test]
    fn test_initialize_empty_signers_fails() {
        let env = Env::default();
        let factory_address = env.register_contract(None, Factory);
        let client = FactoryClient::new(&env, &factory_address);
        let fee_to_setter = Address::generate(&env);

        let pair_wasm_hash = env.deployer().upload_contract_wasm(Bytes::new(&env));
        let lp_token_wasm_hash = env.deployer().upload_contract_wasm(Bytes::new(&env));

        // Empty signers should fail with InvalidSignerCount (error code 4)
        let result = client.try_initialize(
            &Vec::new(&env),
            &pair_wasm_hash,
            &lp_token_wasm_hash,
            &fee_to_setter,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_initialize_too_many_signers_fails() {
        let env = Env::default();
        let factory_address = env.register_contract(None, Factory);
        let client = FactoryClient::new(&env, &factory_address);
        let fee_to_setter = Address::generate(&env);

        let pair_wasm_hash = env.deployer().upload_contract_wasm(Bytes::new(&env));
        let lp_token_wasm_hash = env.deployer().upload_contract_wasm(Bytes::new(&env));

        // 11 signers exceeds the max of 10
        let mut signers = Vec::new(&env);
        for _ in 0..11 {
            signers.push_back(Address::generate(&env));
        }

        let result =
            client.try_initialize(&signers, &pair_wasm_hash, &lp_token_wasm_hash, &fee_to_setter);
        assert!(result.is_err());
    }

    #[test]
    fn test_initialize_single_signer_succeeds() {
        let env = Env::default();
        let factory_address = env.register_contract(None, Factory);
        let client = FactoryClient::new(&env, &factory_address);
        let fee_to_setter = Address::generate(&env);

        let signer = Address::generate(&env);
        let pair_wasm_hash = env.deployer().upload_contract_wasm(Bytes::new(&env));
        let lp_token_wasm_hash = env.deployer().upload_contract_wasm(Bytes::new(&env));

        // 1 signer is the minimum valid count
        client.initialize(
            &Vec::from_array(&env, [signer]),
            &pair_wasm_hash,
            &lp_token_wasm_hash,
            &fee_to_setter,
        );

        assert!(!client.is_paused());
    }

    #[test]
    fn test_initialize_ten_signers_succeeds() {
        let env = Env::default();
        let factory_address = env.register_contract(None, Factory);
        let client = FactoryClient::new(&env, &factory_address);
        let fee_to_setter = Address::generate(&env);

        let pair_wasm_hash = env.deployer().upload_contract_wasm(Bytes::new(&env));
        let lp_token_wasm_hash = env.deployer().upload_contract_wasm(Bytes::new(&env));

        // 10 signers is the maximum valid count
        let mut signers = Vec::new(&env);
        for _ in 0..10 {
            signers.push_back(Address::generate(&env));
        }

        client.initialize(&signers, &pair_wasm_hash, &lp_token_wasm_hash, &fee_to_setter);

        assert!(!client.is_paused());
    }

    // ---------- is_paused after init ----------

    #[test]
    fn test_is_paused_after_init() {
        let (_env, client, _, _, _, _) = setup_env();
        assert!(!client.is_paused());
    }

    // ---------- Existing tests (adapted) ----------

    #[test]
    fn test_double_initialization_fails() {
        let (env, client, _, _, _, fee_to_setter) = setup_env();

        let pair_wasm_hash =
            env.deployer().upload_contract_wasm(Bytes::from_slice(&env, PAIR_WASM));
        let lp_token_wasm_hash =
            env.deployer().upload_contract_wasm(Bytes::from_slice(&env, LP_TOKEN_WASM));

        let result = client.try_initialize(
            &Vec::from_array(
                &env,
                [Address::generate(&env), Address::generate(&env), Address::generate(&env)],
            ),
            &pair_wasm_hash,
            &lp_token_wasm_hash,
            &fee_to_setter,
        );
        assert!(result.is_err());
    }

    // ── create_pair: happy path ──────────────────────────────────────────────

    #[test]
    fn test_create_pair_happy_path() {
        let (_env, client, token_a, token_b, _, _) = setup_env();

        let pair_addr = client.create_pair(&token_a, &token_b);

        // The returned pair address should be retrievable via get_pair.
        let stored = client.get_pair(&token_a, &token_b);
        assert_eq!(stored, Some(pair_addr.clone()));
    }

    #[test]
    fn test_create_pair_reverse_order_returns_same_pair() {
        let (_env, client, token_a, token_b, _, _) = setup_env();

        let pair_addr = client.create_pair(&token_a, &token_b);

        // Querying with reversed token order must return the same pair.
        let stored_reverse = client.get_pair(&token_b, &token_a);
        assert_eq!(stored_reverse, Some(pair_addr));
    }

    #[test]
    fn test_create_pair_canonical_ordering() {
        let (env, client, _, _, _, _) = setup_env();

        let token_x = Address::generate(&env);
        let token_y = Address::generate(&env);

        // Create with (x, y), then verify both orderings resolve.
        let pair_1 = client.create_pair(&token_x, &token_y);
        assert_eq!(client.get_pair(&token_x, &token_y), Some(pair_1.clone()));
        assert_eq!(client.get_pair(&token_y, &token_x), Some(pair_1));
    }

    #[test]
    fn test_create_multiple_pairs() {
        let (env, client, token_a, token_b, _, _) = setup_env();

        let token_c = Address::generate(&env);

        let pair_ab = client.create_pair(&token_a, &token_b);
        let pair_ac = client.create_pair(&token_a, &token_c);
        let pair_bc = client.create_pair(&token_b, &token_c);

        // Each pair should have a distinct address.
        assert_ne!(pair_ab, pair_ac);
        assert_ne!(pair_ab, pair_bc);
        assert_ne!(pair_ac, pair_bc);

        // All pairs should be retrievable.
        assert_eq!(client.get_pair(&token_a, &token_b), Some(pair_ab));
        assert_eq!(client.get_pair(&token_a, &token_c), Some(pair_ac));
        assert_eq!(client.get_pair(&token_b, &token_c), Some(pair_bc));
    }

    // ── create_pair: error paths ─────────────────────────────────────────────

    #[test]
    fn test_create_pair_identical_tokens() {
        let (_env, client, token_a, _token_b, _, _) = setup_env();

        let result = client.try_create_pair(&token_a, &token_a);
        assert!(result.is_err());
    }

    #[test]
    fn test_create_pair_duplicate_returns_error() {
        let (_env, client, token_a, token_b, _, _) = setup_env();

        // First creation succeeds.
        client.create_pair(&token_a, &token_b);

        // Second creation with same tokens must fail (PairExists).
        let result = client.try_create_pair(&token_a, &token_b);
        assert!(result.is_err());
    }

    #[test]
    fn test_create_pair_duplicate_reversed_order() {
        let (_env, client, token_a, token_b, _, _) = setup_env();

        // Create (A, B).
        client.create_pair(&token_a, &token_b);

        // Attempt (B, A) — canonical sort means this is the same pair.
        let result = client.try_create_pair(&token_b, &token_a);
        assert!(result.is_err());
    }

    #[test]
    fn test_create_pair_while_paused() {
        let (env, client, token_a, token_b, _, _) = setup_env();

        // Pause the factory.
        client.pause(&Vec::from_array(
            &env,
            [Address::generate(&env), Address::generate(&env), Address::generate(&env)],
        ));
        assert!(client.is_paused());

        // Creating a pair while paused must fail.
        let result = client.try_create_pair(&token_a, &token_b);
        assert!(result.is_err());
    }

    #[test]
    fn test_create_pair_after_unpause() {
        let (env, client, token_a, token_b, _, _) = setup_env();

        // Pause then unpause.
        let signers = Vec::from_array(
            &env,
            [Address::generate(&env), Address::generate(&env), Address::generate(&env)],
        );
        client.pause(&signers);
        client.unpause(&signers);
        assert!(!client.is_paused());

        // Creating after unpause should succeed.
        let pair_addr = client.create_pair(&token_a, &token_b);
        assert_eq!(client.get_pair(&token_a, &token_b), Some(pair_addr));
    }

    // ── get_pair: edge cases ─────────────────────────────────────────────────

    #[test]
    fn test_get_pair_returns_none_for_missing() {
        let (_env, client, token_a, token_b, _, _) = setup_env();
        assert!(client.get_pair(&token_a, &token_b).is_none());
    }

    // ── Fee management ───────────────────────────────────────────────────────

    #[test]
    fn test_set_fee_to() {
        let (env, client, _, _, _, fee_to_setter) = setup_env();

        let fee_recipient = Address::generate(&env);
        client.set_fee_to(&fee_to_setter, &Some(fee_recipient.clone()));
        assert_eq!(client.fee_to(), Some(fee_recipient));
    }

    #[test]
    fn test_set_fee_to_unauthorized() {
        let (env, client, _, _, _, _) = setup_env();

        let rando = Address::generate(&env);
        let fee_recipient = Address::generate(&env);
        let result = client.try_set_fee_to(&rando, &Some(fee_recipient));
        assert!(result.is_err());
    }

    #[test]
    fn test_set_fee_to_setter() {
        let (env, client, _, _, _, fee_to_setter) = setup_env();

        let new_setter = Address::generate(&env);
        client.set_fee_to_setter(&fee_to_setter, &new_setter);
        assert_eq!(client.fee_to_setter(), Some(new_setter));
    }
}
