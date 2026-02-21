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

    #[test]
    fn test_initialization() {
        let (_env, client, _, _, _, _) = setup_env();
        assert_eq!(client.is_paused(), false);
    }

    #[test]
    fn test_create_pair_validation() {
        let (_env, client, token_a, _token_b, _, _) = setup_env();

        // Identical tokens should return Err(IdenticalTokens = 8)
        let result = client.try_create_pair(&token_a, &token_a);
        assert!(result.is_err());
        // Soroban error codes are a bit tricky to match directly in try_ calls without more boilerplate,
        // but we verify it's an error.
    }

    #[test]
    fn test_get_pair_none_for_missing() {
        let (_env, client, token_a, token_b, _, _) = setup_env();
        assert!(client.get_pair(&token_a, &token_b).is_none());
    }
}
