#![cfg(test)]

use soroban_sdk::Env;

mod factory_tests {
    use super::*;
    use crate::{Factory, FactoryClient};
    use soroban_sdk::{testutils::Address as _, Address, Bytes, Vec};

    fn setup_env<'a>() -> (Env, FactoryClient<'a>, Address, Address, Address, Address) {
        let env = Env::default();
        let factory_address = env.register_contract(None, Factory);
        let client = FactoryClient::new(&env, &factory_address);

        let signer_1 = Address::generate(&env);
        let signer_2 = Address::generate(&env);
        let signer_3 = Address::generate(&env);
        let fee_to_setter = Address::generate(&env);

        let pair_wasm_hash = env.deployer().upload_contract_wasm(Bytes::new(&env));
        let lp_token_wasm_hash = env.deployer().upload_contract_wasm(Bytes::new(&env));

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
        assert_eq!(client.is_paused(), false);
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

        assert_eq!(client.is_paused(), false);
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

        assert_eq!(client.is_paused(), false);
    }

    // ---------- is_paused after init ----------

    #[test]
    fn test_is_paused_after_init() {
        let (_env, client, _, _, _, _) = setup_env();
        assert_eq!(client.is_paused(), false);
    }

    // ---------- Existing tests (adapted) ----------

    #[test]
    fn test_create_pair_validation() {
        let (_env, client, token_a, _token_b, _, _) = setup_env();

        // Identical tokens should return Err(IdenticalTokens = 8)
        let result = client.try_create_pair(&token_a, &token_a);
        assert!(result.is_err());
    }

    #[test]
    fn test_get_pair_none_for_missing() {
        let (_env, client, token_a, token_b, _, _) = setup_env();
        assert!(client.get_pair(&token_a, &token_b).is_none());
    }
}
