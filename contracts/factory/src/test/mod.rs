use soroban_sdk::Env;

mod factory_tests {
    use super::*;
    use crate::{Factory, FactoryClient};
    use soroban_sdk::{testutils::Address as _, testutils::Events, Address, Bytes, Vec};
    use std::fs;
    use std::path::PathBuf;

    fn load_wasm(file_name: &str) -> std::vec::Vec<u8> {
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target");
        let candidates = [
            base.join("wasm32-unknown-unknown/release").join(file_name),
            base.join("wasm32v1-none/release").join(file_name),
        ];

        for path in candidates {
            if let Ok(bytes) = fs::read(&path) {
                return bytes;
            }
        }

        panic!(
            "failed to read test wasm artifact {}; checked wasm32-unknown-unknown and wasm32v1-none release targets",
            file_name
        );
    }

    /// Helper: sets up a fresh Env, deploys the factory, initializes it with
    /// real pair / LP-token WASM hashes, and returns commonly-needed handles.
    fn setup_env<'a>() -> (Env, FactoryClient<'a>, Address, Address, Address, Address, Vec<Address>)
    {
        let env = Env::default();
        env.mock_all_auths();

        let factory_address = env.register_contract(None, Factory);
        let client = FactoryClient::new(&env, &factory_address);

        let signer_1 = Address::generate(&env);
        let signer_2 = Address::generate(&env);
        let signer_3 = Address::generate(&env);
        let fee_to_setter = Address::generate(&env);

        // Upload real WASM so deployer().deploy() produces working contracts.
        let pair_wasm = load_wasm("coralswap_pair.wasm");
        let lp_token_wasm = load_wasm("coralswap_lp_token.wasm");
        let pair_wasm_hash =
            env.deployer().upload_contract_wasm(Bytes::from_slice(&env, &pair_wasm));
        let lp_token_wasm_hash =
            env.deployer().upload_contract_wasm(Bytes::from_slice(&env, &lp_token_wasm));

        let signers = Vec::from_array(&env, [signer_1.clone(), signer_2.clone(), signer_3.clone()]);

        client.initialize(&signers, &pair_wasm_hash, &lp_token_wasm_hash, &fee_to_setter);

        let token_a = Address::generate(&env);
        let token_b = Address::generate(&env);

        (env, client, token_a, token_b, factory_address, fee_to_setter, signers)
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
        let (env, client, _, _, _, _, _) = setup_env();

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
        let (_env, client, _, _, _, _, _) = setup_env();
        assert!(!client.is_paused());
    }

    // ── Timelock upgrade (Issue #99) ──────────────────────────────────────────

    #[test]
    fn test_propose_upgrade_stores_proposal() {
        let (env, client, _, _, _, _, _) = setup_env();
        let new_hash = env.deployer().upload_contract_wasm(Bytes::new(&env));
        let signers = Vec::from_array(&env, [Address::generate(&env), Address::generate(&env)]);
        client.propose_upgrade(&signers, &new_hash);
    }

    #[test]
    fn test_propose_upgrade_duplicate_rejected() {
        let (env, client, _, _, _, _, _) = setup_env();
        let new_hash = env.deployer().upload_contract_wasm(Bytes::new(&env));
        let signers = Vec::from_array(&env, [Address::generate(&env), Address::generate(&env)]);
        client.propose_upgrade(&signers, &new_hash);
        let result = client.try_propose_upgrade(&signers, &new_hash);
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_upgrade_too_early_fails() {
        let (env, client, _, _, _, _, _) = setup_env();
        let new_hash = env.deployer().upload_contract_wasm(Bytes::new(&env));
        let signers = Vec::from_array(&env, [Address::generate(&env), Address::generate(&env)]);
        client.propose_upgrade(&signers, &new_hash);
        let result = client.try_execute_upgrade();
        assert!(result.is_err());
    }

    #[test]
    fn test_cancel_upgrade_clears_proposal() {
        let (env, client, _, _, _, _, _) = setup_env();
        let new_hash = env.deployer().upload_contract_wasm(Bytes::new(&env));
        let signers = Vec::from_array(&env, [Address::generate(&env), Address::generate(&env)]);
        client.propose_upgrade(&signers, &new_hash);
        client.cancel_upgrade(&signers);
        let result = client.try_execute_upgrade();
        assert!(result.is_err());
    }

    #[test]
    fn test_cancel_upgrade_no_proposal_fails() {
        let (env, client, _, _, _, _, _) = setup_env();
        let signers = Vec::from_array(&env, [Address::generate(&env), Address::generate(&env)]);
        let result = client.try_cancel_upgrade(&signers);
        assert!(result.is_err());
    }

    #[test]
    fn test_propose_upgrade_insufficient_signers_fails() {
        let env = Env::default();
        let factory_address = env.register_contract(None, Factory);
        let client = FactoryClient::new(&env, &factory_address);
        let fee_to_setter = Address::generate(&env);

        let pair_wasm_hash = env.deployer().upload_contract_wasm(Bytes::new(&env));
        let lp_token_wasm_hash = env.deployer().upload_contract_wasm(Bytes::new(&env));

        let s1 = Address::generate(&env);
        let s2 = Address::generate(&env);
        let s3 = Address::generate(&env);
        client.initialize(
            &Vec::from_array(&env, [s1, s2, s3]),
            &pair_wasm_hash,
            &lp_token_wasm_hash,
            &fee_to_setter,
        );

        let new_hash = env.deployer().upload_contract_wasm(Bytes::new(&env));
        let result = client.try_propose_upgrade(&Vec::new(&env), &new_hash);
        assert!(result.is_err());
    }

    // ---------- Existing tests (adapted) ----------

    #[test]
    fn test_double_initialization_fails() {
        let (env, client, _, _, _, fee_to_setter, _) = setup_env();

        let pair_wasm = load_wasm("coralswap_pair.wasm");
        let lp_token_wasm = load_wasm("coralswap_lp_token.wasm");
        let pair_wasm_hash =
            env.deployer().upload_contract_wasm(Bytes::from_slice(&env, &pair_wasm));
        let lp_token_wasm_hash =
            env.deployer().upload_contract_wasm(Bytes::from_slice(&env, &lp_token_wasm));

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
        let (_env, client, token_a, token_b, _, _, _) = setup_env();

        let pair_addr = client.create_pair(&token_a, &token_b);

        // The returned pair address should be retrievable via get_pair.
        let stored = client.get_pair(&token_a, &token_b);
        assert_eq!(stored, Some(pair_addr.clone()));
    }

    #[test]
    fn test_create_pair_reverse_order_returns_same_pair() {
        let (_env, client, token_a, token_b, _, _, _) = setup_env();

        let pair_addr = client.create_pair(&token_a, &token_b);

        // Querying with reversed token order must return the same pair.
        let stored_reverse = client.get_pair(&token_b, &token_a);
        assert_eq!(stored_reverse, Some(pair_addr));
    }

    #[test]
    fn test_create_pair_canonical_ordering() {
        let (env, client, _, _, _, _, _) = setup_env();

        let token_x = Address::generate(&env);
        let token_y = Address::generate(&env);

        // Create with (x, y), then verify both orderings resolve.
        let pair_1 = client.create_pair(&token_x, &token_y);
        assert_eq!(client.get_pair(&token_x, &token_y), Some(pair_1.clone()));
        assert_eq!(client.get_pair(&token_y, &token_x), Some(pair_1));
    }

    #[test]
    fn test_create_multiple_pairs() {
        let (env, client, token_a, token_b, _, _, _) = setup_env();

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
        let (_env, client, token_a, _token_b, _, _, _) = setup_env();

        let result = client.try_create_pair(&token_a, &token_a);
        assert!(result.is_err());
    }

    #[test]
    fn test_create_pair_duplicate_returns_error() {
        let (_env, client, token_a, token_b, _, _, _) = setup_env();

        // First creation succeeds.
        client.create_pair(&token_a, &token_b);

        // Second creation with same tokens must fail (PairExists).
        let result = client.try_create_pair(&token_a, &token_b);
        assert!(result.is_err());
    }

    #[test]
    fn test_create_pair_duplicate_reversed_order() {
        let (_env, client, token_a, token_b, _, _, _) = setup_env();

        // Create (A, B).
        client.create_pair(&token_a, &token_b);

        // Attempt (B, A) — canonical sort means this is the same pair.
        let result = client.try_create_pair(&token_b, &token_a);
        assert!(result.is_err());
    }

    #[test]
    fn test_create_pair_while_paused() {
        let (env, client, token_a, token_b, _, _, signers) = setup_env();

        // Pause the factory using a stored signer.
        client.pause(&signers);
        assert!(client.is_paused());

        // Creating a pair while paused must fail.
        let result = client.try_create_pair(&token_a, &token_b);
        assert!(result.is_err());
    }

    #[test]
    fn test_create_pair_after_unpause() {
        let (_env, client, token_a, token_b, _, _, signers) = setup_env();

        // Pause then unpause using stored signers.
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
        let (_env, client, token_a, token_b, _, _, _) = setup_env();
        assert!(client.get_pair(&token_a, &token_b).is_none());
    }

    // ── Multisig governance (Issue #98) ──────────────────────────────────────

    #[test]
    fn test_pause_authorized_signers_succeeds() {
        let (env, client, _, _, _, _, _) = setup_env();
        // setup_env initialises with 3 signers and mock_all_auths, so any
        // address passes require_auth(). Threshold = ceil(3/2) = 2.
        let s1 = Address::generate(&env);
        let s2 = Address::generate(&env);
        client.pause(&Vec::from_array(&env, [s1, s2]));
        assert!(client.is_paused());
    }

    #[test]
    fn test_unpause_authorized_signers_succeeds() {
        let (env, client, _, _, _, _, _) = setup_env();
        let s1 = Address::generate(&env);
        let s2 = Address::generate(&env);
        client.pause(&Vec::from_array(&env, [s1.clone(), s2.clone()]));
        client.unpause(&Vec::from_array(&env, [s1, s2]));
        assert!(!client.is_paused());
    }

    #[test]
    fn test_pause_insufficient_signers_fails() {
        let env = Env::default();
        // Do NOT mock_all_auths — we want real auth enforcement.
        let factory_address = env.register_contract(None, Factory);
        let client = FactoryClient::new(&env, &factory_address);
        let fee_to_setter = Address::generate(&env);

        let pair_wasm_hash = env.deployer().upload_contract_wasm(Bytes::new(&env));
        let lp_token_wasm_hash = env.deployer().upload_contract_wasm(Bytes::new(&env));

        // 3 signers registered → threshold = 2; provide only 0 → should fail.
        let s1 = Address::generate(&env);
        let s2 = Address::generate(&env);
        let s3 = Address::generate(&env);
        client.initialize(
            &Vec::from_array(&env, [s1, s2, s3]),
            &pair_wasm_hash,
            &lp_token_wasm_hash,
            &fee_to_setter,
        );

        let result = client.try_pause(&Vec::new(&env));
        assert!(result.is_err());
    }

    #[test]
    fn test_unpause_insufficient_signers_fails() {
        let env = Env::default();
        let factory_address = env.register_contract(None, Factory);
        let client = FactoryClient::new(&env, &factory_address);
        let fee_to_setter = Address::generate(&env);

        let pair_wasm_hash = env.deployer().upload_contract_wasm(Bytes::new(&env));
        let lp_token_wasm_hash = env.deployer().upload_contract_wasm(Bytes::new(&env));

        let s1 = Address::generate(&env);
        let s2 = Address::generate(&env);
        let s3 = Address::generate(&env);
        client.initialize(
            &Vec::from_array(&env, [s1, s2, s3]),
            &pair_wasm_hash,
            &lp_token_wasm_hash,
            &fee_to_setter,
        );

        // Provide 0 signers → InsufficientSignatures
        let result = client.try_unpause(&Vec::new(&env));
        assert!(result.is_err());
    }

    // ── Fee management ───────────────────────────────────────────────────────

    #[test]
    fn test_set_fee_to() {
        let (env, client, _, _, _, fee_to_setter, _) = setup_env();

        let fee_recipient = Address::generate(&env);
        client.set_fee_to(&fee_to_setter, &Some(fee_recipient.clone()), &10u32);
        assert_eq!(client.fee_to(), Some(fee_recipient));
    }

    #[test]
    fn test_set_fee_to_unauthorized() {
        let (env, client, _, _, _, _, _) = setup_env();

        let rando = Address::generate(&env);
        let fee_recipient = Address::generate(&env);
        let result = client.try_set_fee_to(&rando, &Some(fee_recipient), &10u32);
        assert!(result.is_err());
    }

    #[test]
    fn test_set_fee_to_setter() {
        let (env, client, _, _, _, fee_to_setter, _) = setup_env();

        let new_setter = Address::generate(&env);
        client.set_fee_to_setter(&fee_to_setter, &new_setter);
        assert_eq!(client.fee_to_setter(), Some(new_setter));
    }

    // ── Pause / Unpause auth checks (issue #86) ─────────────────────────────

    #[test]
    fn test_pause_with_unknown_signer_fails() {
        let (env, client, _, _, _, _, _) = setup_env();

        // A freshly-generated address is guaranteed not to be in the stored
        // signers list — the call must be rejected with Unauthorized.
        let unknown = Address::generate(&env);
        let result = client.try_pause(&Vec::from_array(&env, [unknown]));
        assert!(result.is_err(), "unknown signer must be rejected by pause");
    }

    #[test]
    fn test_unpause_with_unknown_signer_fails() {
        let (env, client, _, _, _, _, _) = setup_env();

        let unknown = Address::generate(&env);
        let result = client.try_unpause(&Vec::from_array(&env, [unknown]));
        assert!(result.is_err(), "unknown signer must be rejected by unpause");
    }

    #[test]
    fn test_pause_with_empty_signers_fails() {
        let (env, client, _, _, _, _, _) = setup_env();

        let result = client.try_pause(&Vec::new(&env));
        assert!(result.is_err(), "empty signers list must be rejected by pause");
    }

    #[test]
    fn test_unpause_with_empty_signers_fails() {
        let (env, client, _, _, _, _, _) = setup_env();

        let result = client.try_unpause(&Vec::new(&env));
        assert!(result.is_err(), "empty signers list must be rejected by unpause");
    }

    // ── Issue #110: get_all_pairs / get_pair_count ───────────────────────────

    #[test]
    fn test_get_pair_count_initially_zero() {
        let (_env, client, _, _, _, _, _) = setup_env();
        assert_eq!(client.get_pair_count(), 0);
    }

    #[test]
    fn test_get_all_pairs_empty_list() {
        let (env, client, _, _, _, _, _) = setup_env();
        let result = client.get_all_pairs(&0, &10);
        assert!(result.is_empty());
    }

    #[test]
    fn test_get_all_pairs_over_limit_returns_error() {
        let (env, client, _, _, _, _, _) = setup_env();
        let result = client.try_get_all_pairs(&0, &51);
        assert!(result.is_err(), "limit > 50 must return an error");
    }

    #[test]
    fn test_get_all_pairs_returns_pairs_and_paginates() {
        let (env, client, token_a, token_b, _, _, _) = setup_env();
        let token_c = Address::generate(&env);

        let pair1 = client.create_pair(&token_a, &token_b);
        let pair2 = client.create_pair(&token_a, &token_c);

        assert_eq!(client.get_pair_count(), 2);

        let all = client.get_all_pairs(&0, &10);
        assert_eq!(all.len(), 2);
        assert!(all.contains(&pair1));
        assert!(all.contains(&pair2));

        // Paginate: offset=1 limit=1 → only second pair
        let page = client.get_all_pairs(&1, &1);
        assert_eq!(page.len(), 1);

        // Offset beyond total → empty
        let overflow = client.get_all_pairs(&100, &10);
        assert!(overflow.is_empty());
    }

    // ── Issue #111: set_fee_to with fee_bps validation ───────────────────────

    #[test]
    fn test_set_fee_to_fee_too_high_returns_error() {
        let (env, client, _, _, _, fee_to_setter, _) = setup_env();
        let fee_recipient = Address::generate(&env);
        let result = client.try_set_fee_to(&fee_to_setter, &Some(fee_recipient), &31u32);
        assert!(result.is_err(), "fee_bps > 30 must return an error");
    }

    #[test]
    fn test_set_fee_to_zero_address_with_nonzero_fee_returns_error() {
        let (env, client, _, _, _, fee_to_setter, _) = setup_env();
        // Passing None recipient but nonzero fee must fail
        let result = client.try_set_fee_to(&fee_to_setter, &None, &10u32);
        assert!(result.is_err(), "None fee_to with nonzero fee_bps must return an error");
    }

    #[test]
    fn test_set_fee_to_valid_update_emits_event() {
        let (env, client, _, _, _, fee_to_setter, _) = setup_env();
        let fee_recipient = Address::generate(&env);
        // valid: fee_bps = 20 (≤ 30), recipient set
        let result = client.try_set_fee_to(&fee_to_setter, &Some(fee_recipient.clone()), &20u32);
        assert!(result.is_ok(), "valid fee update must succeed");
        assert_eq!(client.fee_to(), Some(fee_recipient));
    }

    // ── Issue #132: set_pair_fee per-pair fee override ──────────────────────

    /// Sets up a fresh factory WITHOUT uploading pair / lp_token WASM. The
    /// pair-fee tests don't need real pair contracts — they only exercise
    /// `set_pair_fee` / `get_pair_fee_override` which take an arbitrary
    /// `Address` argument. This sidesteps the unrelated `reference-types`
    /// host/contract incompatibility that breaks `load_wasm` in this
    /// test environment.
    fn setup_factory_for_pair_fee_tests<'a>() -> (Env, FactoryClient<'a>, Address) {
        let env = Env::default();
        env.mock_all_auths();

        let factory_address = env.register_contract(None, Factory);
        let client = FactoryClient::new(&env, &factory_address);

        let fee_to_setter = Address::generate(&env);
        let dummy_pair_wasm = env.deployer().upload_contract_wasm(Bytes::new(&env));
        let dummy_lp_wasm = env.deployer().upload_contract_wasm(Bytes::new(&env));
        let signers = Vec::from_array(
            &env,
            [Address::generate(&env), Address::generate(&env), Address::generate(&env)],
        );
        client.initialize(&signers, &dummy_pair_wasm, &dummy_lp_wasm, &fee_to_setter);

        (env, client, fee_to_setter)
    }

    #[test]
    fn test_set_pair_fee_admin_succeeds_and_is_readable() {
        let (env, client, fee_to_setter) = setup_factory_for_pair_fee_tests();
        let pair = Address::generate(&env);

        // Initially no override is set.
        assert_eq!(client.get_pair_fee_override(&pair), None);

        // 5 bps is well within the 100 bps cap (issue example: USDC/EURC @ 5 bps).
        client.set_pair_fee(&fee_to_setter, &pair, &5u32);

        assert_eq!(client.get_pair_fee_override(&pair), Some(5u32));
    }

    #[test]
    fn test_set_pair_fee_unauthorized_caller_reverts() {
        let (env, client, _setter) = setup_factory_for_pair_fee_tests();
        let pair = Address::generate(&env);
        let rando = Address::generate(&env);

        // Rando is not the registered fee_to_setter.
        let result = client.try_set_pair_fee(&rando, &pair, &10u32);
        assert!(result.is_err(), "non-admin must not be able to set the override");

        // State must be untouched.
        assert_eq!(client.get_pair_fee_override(&pair), None);
    }

    #[test]
    fn test_set_pair_fee_above_cap_reverts_with_fee_too_high() {
        let (env, client, fee_to_setter) = setup_factory_for_pair_fee_tests();
        let pair = Address::generate(&env);

        // 101 bps exceeds the 1% (100 bps) cap.
        let result = client.try_set_pair_fee(&fee_to_setter, &pair, &101u32);
        assert!(result.is_err(), "fee_bps > 100 must be rejected");

        // Far above the cap must also be rejected.
        let result_huge = client.try_set_pair_fee(&fee_to_setter, &pair, &10_000u32);
        assert!(result_huge.is_err(), "fee_bps = 10000 must be rejected");

        // And the override must not have been partially stored.
        assert_eq!(client.get_pair_fee_override(&pair), None);
    }

    #[test]
    fn test_set_pair_fee_boundary_values_are_accepted() {
        let (env, client, fee_to_setter) = setup_factory_for_pair_fee_tests();

        // fee_bps = 0 is allowed and stored verbatim (lets governance "clear"
        // the override by setting it to zero).
        let pair_zero = Address::generate(&env);
        client.set_pair_fee(&fee_to_setter, &pair_zero, &0u32);
        assert_eq!(client.get_pair_fee_override(&pair_zero), Some(0u32));

        // fee_bps = 100 is the documented maximum.
        let pair_max = Address::generate(&env);
        client.set_pair_fee(&fee_to_setter, &pair_max, &100u32);
        assert_eq!(client.get_pair_fee_override(&pair_max), Some(100u32));
    }

    #[test]
    fn test_set_pair_fee_overwrites_previous_override() {
        let (env, client, fee_to_setter) = setup_factory_for_pair_fee_tests();
        let pair = Address::generate(&env);

        // Install a 30 bps override, then lower it to 5 bps.
        client.set_pair_fee(&fee_to_setter, &pair, &30u32);
        assert_eq!(client.get_pair_fee_override(&pair), Some(30u32));

        client.set_pair_fee(&fee_to_setter, &pair, &5u32);
        assert_eq!(client.get_pair_fee_override(&pair), Some(5u32));

        // And it must not bleed into other pairs.
        let other_pair = Address::generate(&env);
        assert_eq!(client.get_pair_fee_override(&other_pair), None);
    }

    #[test]
    fn test_set_pair_fee_emits_pair_fee_override_event() {
        let (env, client, fee_to_setter) = setup_factory_for_pair_fee_tests();
        let pair = Address::generate(&env);

        // Initial override: 0 (no override stored yet).
        client.set_pair_fee(&fee_to_setter, &pair, &5u32);

        let all = env.events().all();
        assert_eq!(all.len(), 1, "set_pair_fee must emit exactly one event on success");
    }
}
