#[cfg(test)]
mod integration_tests {
    use soroban_sdk::{
        contractclient,
        testutils::Address as _,
        token::{StellarAssetClient, TokenClient},
        Address, Bytes, BytesN, Env, Vec,
    };
    use std::{fs, path::PathBuf};

    #[contractclient(name = "FactoryClient")]
    pub trait FactoryInterface {
        fn initialize(
            env: Env,
            signers: Vec<Address>,
            pair_wasm_hash: BytesN<32>,
            lp_token_wasm_hash: BytesN<32>,
            fee_to_setter: Address,
        );
        fn create_pair(env: Env, token_a: Address, token_b: Address) -> Address;
        fn get_pair(env: Env, token_a: Address, token_b: Address) -> Option<Address>;
    }

    #[contractclient(name = "PairClient")]
    pub trait PairInterface {
        fn mint(env: Env, to: Address) -> i128;
        fn swap(env: Env, amount_a_out: i128, amount_b_out: i128, to: Address);
        fn get_reserves(env: Env) -> (i128, i128, u64);
        fn lp_token(env: Env) -> Address;
        fn get_current_fee_bps(env: Env) -> u32;
    }

    #[contractclient(name = "RouterClient")]
    pub trait RouterInterface {
        fn initialize(env: Env, factory: Address);
        fn add_liquidity(
            env: Env,
            token_a: Address,
            token_b: Address,
            amount_a_desired: i128,
            amount_b_desired: i128,
            amount_a_min: i128,
            amount_b_min: i128,
            to: Address,
            deadline: u64,
        ) -> (i128, i128, i128);
        fn remove_liquidity(
            env: Env,
            token_a: Address,
            token_b: Address,
            liquidity: i128,
            amount_a_min: i128,
            amount_b_min: i128,
            to: Address,
            deadline: u64,
        ) -> (i128, i128);
    }

    fn load_wasm(file_name: &str) -> Vec<u8> {
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target");
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

    fn compute_amount_out(amount_in: i128, reserve_in: i128, reserve_out: i128, fee_bps: u32) -> i128 {
        let amount_in_with_fee = amount_in * (10000 - fee_bps as i128);
        let numerator = amount_in_with_fee * reserve_out;
        let denominator = reserve_in * 10000 + amount_in_with_fee;
        numerator / denominator
    }

    #[test]
    fn test_full_coral_swap_flow() {
        let env = Env::default();
        env.mock_all_auths_allowing_non_root_auth();

        let admin = Address::generate(&env);
        let user = Address::generate(&env);
        let fee_to_setter = Address::generate(&env);

        let token_a = env.register_stellar_asset_contract(admin.clone());
        let token_b = env.register_stellar_asset_contract(admin.clone());
        let (token_a, token_b) = if token_a < token_b {
            (token_a, token_b)
        } else {
            (token_b, token_a)
        };

        let factory = env.register_contract_wasm(None, load_wasm("coralswap_factory.wasm"));
        let router = env.register_contract_wasm(None, load_wasm("coralswap_router.wasm"));

        let pair_wasm_hash = env.deployer().upload_contract_wasm(Bytes::from_slice(&env, &load_wasm("coralswap_pair.wasm")));
        let lp_token_wasm_hash = env.deployer().upload_contract_wasm(Bytes::from_slice(&env, &load_wasm("coralswap_lp_token.wasm")));

        let factory_client = FactoryClient::new(&env, &factory);
        let signers = Vec::from_array(
            &env,
            [
                Address::generate(&env),
                Address::generate(&env),
                Address::generate(&env),
            ],
        );
        factory_client.initialize(&signers, &pair_wasm_hash, &lp_token_wasm_hash, &fee_to_setter);

        let pair_address = factory_client.create_pair(&token_a, &token_b);
        assert_eq!(factory_client.get_pair(&token_a, &token_b), Some(pair_address.clone()));

        let router_client = RouterClient::new(&env, &router);
        router_client.initialize(factory.clone());

        let token_a_admin = StellarAssetClient::new(&env, &token_a);
        let token_b_admin = StellarAssetClient::new(&env, &token_b);
        let token_a_client = TokenClient::new(&env, &token_a);

        let deposit_a = 1_000_000_i128;
        let deposit_b = 2_000_000_i128;
        token_a_admin.mint(&user, &deposit_a);
        token_b_admin.mint(&user, &deposit_b);

        let deadline = env.ledger().timestamp() + 100;
        let (amount_a, amount_b, liquidity) = router_client.add_liquidity(
            token_a.clone(),
            token_b.clone(),
            deposit_a,
            deposit_b,
            deposit_a,
            deposit_b,
            user.clone(),
            deadline,
        );

        assert_eq!(amount_a, deposit_a);
        assert_eq!(amount_b, deposit_b);
        assert!(liquidity > 0, "liquidity must be minted for first deposit");

        let pair_client = PairClient::new(&env, &pair_address);
        let (reserve_a, reserve_b, _) = pair_client.get_reserves();
        assert_eq!(reserve_a, deposit_a);
        assert_eq!(reserve_b, deposit_b);

        let lp_token_address = pair_client.lp_token();
        let lp_token_client = TokenClient::new(&env, &lp_token_address);
        assert_eq!(lp_token_client.balance(&user), liquidity);
        assert_eq!(lp_token_client.balance(&pair_address), 1_000_i128);

        let previous_k = reserve_a.checked_mul(reserve_b).expect("k overflow");

        let swap_in = 100_000_i128;
        token_a_admin.mint(&user, &swap_in);
        token_a_client.transfer(&user, &pair_address, &swap_in);

        let fee_bps = pair_client.get_current_fee_bps();
        let amount_b_out = compute_amount_out(swap_in, reserve_a, reserve_b, fee_bps);
        assert!(amount_b_out > 0, "swap output should be positive");

        pair_client.swap(&0, &amount_b_out, &user);

        let (reserve_a2, reserve_b2, _) = pair_client.get_reserves();
        assert!(reserve_a2 > reserve_a, "reserve_a should increase after receiving token_a input");
        assert!(reserve_b2 < reserve_b, "reserve_b should decrease after sending token_b output");

        let new_k = reserve_a2.checked_mul(reserve_b2).expect("k overflow");
        assert!(new_k >= previous_k, "k invariant must be preserved after fee-adjusted swap");

        let (returned_a, returned_b) = router_client.remove_liquidity(
            token_a.clone(),
            token_b.clone(),
            liquidity,
            0,
            0,
            user.clone(),
            deadline,
        );

        assert!(returned_a > 0, "remove_liquidity must return token_a");
        assert!(returned_b > 0, "remove_liquidity must return token_b");
        assert_eq!(lp_token_client.balance(&user), 0);
        assert_eq!(lp_token_client.balance(&pair_address), 1_000_i128);
    }
}
