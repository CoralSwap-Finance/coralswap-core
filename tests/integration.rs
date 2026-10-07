#[cfg(test)]
#[allow(dead_code, deprecated)]
mod integration_tests {
    use soroban_sdk::{
        contractclient,
        testutils::Address as _,
        token::{StellarAssetClient, TokenClient},
        Address, Bytes, BytesN, Env, Vec,
    };
    use std::{fs, path::PathBuf, vec::Vec as StdVec};

    #[contractclient(name = "FactoryClient")]
    pub trait FactoryInterface {
        fn initialize(
            env: Env,
            signers: Vec<Address>,
            pair_wasm_hash: BytesN<32>,
            concentrated_pair_wasm_hash: BytesN<32>,
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
        fn initialize(env: Env, factory: Address, hubs: Vec<Address>);
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
            deadline_ledger: Option<u32>,
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
            deadline_ledger: Option<u32>,
        ) -> (i128, i128);
        fn swap_exact_tokens_for_tokens(
            env: Env,
            amount_in: i128,
            amount_out_min: i128,
            path: Vec<Address>,
            to: Address,
            deadline: u64,
            deadline_ledger: Option<u32>,
        ) -> Vec<i128>;
        fn swap_tokens_for_exact_tokens(
            env: Env,
            amount_out: i128,
            amount_in_max: i128,
            path: Vec<Address>,
            to: Address,
            deadline: u64,
            deadline_ledger: Option<u32>,
        ) -> Vec<i128>;
    }

    fn load_wasm(file_name: &str) -> StdVec<u8> {
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("tests crate must be inside the workspace")
            .join("target");
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

    fn compute_amount_out(
        amount_in: i128,
        reserve_in: i128,
        reserve_out: i128,
        fee_bps: u32,
    ) -> i128 {
        let amount_in_with_fee = amount_in * (10000 - fee_bps as i128);
        let numerator = amount_in_with_fee * reserve_out;
        let denominator = reserve_in * 10000 + amount_in_with_fee;
        numerator / denominator
    }

    #[test]
    fn test_full_coral_swap_flow() {
        let env = Env::default();
        // BLANKET MOCK (issue #314): end-to-end wiring test; the guards are
        // covered by the per-contract `auth_matrix` modules.
        env.mock_all_auths_allowing_non_root_auth();
        // Same test-only budget lift as `deploy_with_router_and_pairs`: the full
        // factory + pair + LP-token + router WASM flow outgrows the default test
        // CPU budget; on-chain limits still apply to the deployed contracts.
        env.budget().reset_unlimited();
        env.cost_estimate().disable_resource_limits();

        let admin = Address::generate(&env);
        let user = Address::generate(&env);
        let fee_to_setter = Address::generate(&env);

        let token_a = env.register_stellar_asset_contract_v2(admin.clone()).address();
        let token_b = env.register_stellar_asset_contract_v2(admin.clone()).address();
        let (token_a, token_b) =
            if token_a < token_b { (token_a, token_b) } else { (token_b, token_a) };

        let factory_wasm = load_wasm("coralswap_factory.wasm");
        let factory = env.register_contract_wasm(None, Bytes::from_slice(&env, &factory_wasm));
        let router_wasm = load_wasm("coralswap_router.wasm");
        let router = env.register_contract_wasm(None, Bytes::from_slice(&env, &router_wasm));

        let pair_wasm_hash = env
            .deployer()
            .upload_contract_wasm(Bytes::from_slice(&env, &load_wasm("coralswap_pair.wasm")));
        let lp_token_wasm_hash = env
            .deployer()
            .upload_contract_wasm(Bytes::from_slice(&env, &load_wasm("coralswap_lp_token.wasm")));

        let factory_client = FactoryClient::new(&env, &factory);
        let signers = Vec::from_array(
            &env,
            [Address::generate(&env), Address::generate(&env), Address::generate(&env)],
        );
        factory_client.initialize(
            &signers,
            &pair_wasm_hash,
            &pair_wasm_hash,
            &lp_token_wasm_hash,
            &fee_to_setter,
        );

        let pair_address = factory_client.create_pair(&token_a, &token_b);
        assert_eq!(factory_client.get_pair(&token_a, &token_b), Some(pair_address.clone()));

        let router_client = RouterClient::new(&env, &router);
        router_client.initialize(&factory, &Vec::new(&env));

        let token_a_admin = StellarAssetClient::new(&env, &token_a);
        let token_b_admin = StellarAssetClient::new(&env, &token_b);
        let token_a_client = TokenClient::new(&env, &token_a);

        let deposit_a = 1_000_000_i128;
        let deposit_b = 2_000_000_i128;
        token_a_admin.mint(&user, &deposit_a);
        token_b_admin.mint(&user, &deposit_b);

        let deadline = env.ledger().timestamp() + 100;
        let (amount_a, amount_b, liquidity) = router_client.add_liquidity(
            &token_a, &token_b, &deposit_a, &deposit_b, &deposit_a, &deposit_b, &user, &deadline,
            &None,
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

        let (returned_a, returned_b) = router_client
            .remove_liquidity(&token_a, &token_b, &liquidity, &0, &0, &user, &deadline, &None);

        assert!(returned_a > 0, "remove_liquidity must return token_a");
        assert!(returned_b > 0, "remove_liquidity must return token_b");
        assert_eq!(lp_token_client.balance(&user), 0);
        // The MINIMUM_LIQUIDITY seed remains permanently locked in the pair.
        assert_eq!(lp_token_client.balance(&pair_address), 1_000_i128);
    }

    // =========================================================================
    // Router forward dust — issue #352
    // =========================================================================

    /// Deploys factory + router from the built WASM artifacts, registers `n`
    /// SEP-41 (Stellar Asset) tokens inside the SAME env, creates a pair per
    /// `edges` entry, and funds both reserves via `add_liquidity`.
    ///
    /// Returns the env, the factory, the router, and the token addresses in
    /// registration order (indices per the caller's requested count).
    fn deploy_with_router_and_pairs(
        token_count: u32,
        edges: &[(u32, u32)],
        deposits: i128,
    ) -> (Env, Address, Address, StdVec<Address>) {
        let env = Env::default();
        // BLANKET MOCK (issue #314): end-to-end wiring test; the guards are
        // covered by the per-contract `auth_matrix` modules.
        env.mock_all_auths_allowing_non_root_auth();
        // The fixed default test CPU budget and mainnet invocation limits are
        // sized for single-contract flows; deploying factory + pair + LP-token
        // + router WASM and then routing a multi-hop swap across them exceeds
        // them in the test harness. Test-only lift — the deployed contracts
        // still enforce their own limits on-chain.
        env.budget().reset_unlimited();
        env.cost_estimate().disable_resource_limits();

        let user = Address::generate(&env);
        let fee_to_setter = Address::generate(&env);

        let factory_wasm = load_wasm("coralswap_factory.wasm");
        let factory = env.register_contract_wasm(None, Bytes::from_slice(&env, &factory_wasm));
        let router_wasm = load_wasm("coralswap_router.wasm");
        let router = env.register_contract_wasm(None, Bytes::from_slice(&env, &router_wasm));

        let pair_wasm_hash = env
            .deployer()
            .upload_contract_wasm(Bytes::from_slice(&env, &load_wasm("coralswap_pair.wasm")));
        let lp_token_wasm_hash = env
            .deployer()
            .upload_contract_wasm(Bytes::from_slice(&env, &load_wasm("coralswap_lp_token.wasm")));

        let factory_client = FactoryClient::new(&env, &factory);
        let signers = Vec::from_array(
            &env,
            [Address::generate(&env), Address::generate(&env), Address::generate(&env)],
        );
        factory_client.initialize(
            &signers,
            &pair_wasm_hash,
            &pair_wasm_hash,
            &lp_token_wasm_hash,
            &fee_to_setter,
        );

        let router_client = RouterClient::new(&env, &router);
        router_client.initialize(&factory, &Vec::new(&env));

        // Register all token contracts inside THIS env so their addresses are
        // valid contracts here (addresses generated in a foreign env would hit
        // MissingValue on the first token call).
        let tokens: StdVec<Address> = (0..token_count)
            .map(|_| env.register_stellar_asset_contract_v2(Address::generate(&env)).address())
            .collect();

        let deadline = env.ledger().timestamp() + 100;
        for (a, b) in edges {
            let token_a = tokens.get(*a as usize).unwrap();
            let token_b = tokens.get(*b as usize).unwrap();
            factory_client.create_pair(token_a, token_b);
            StellarAssetClient::new(&env, token_a).mint(&user, &deposits);
            StellarAssetClient::new(&env, token_b).mint(&user, &deposits);
            router_client.add_liquidity(
                token_a, token_b, &deposits, &deposits, &deposits, &deposits, &user, &deadline,
                &None,
            );
        }

        (env, factory, router, tokens)
    }

    /// Constant-product amount-out mirrored from the pair contract.
    fn compute_amount_in(
        amount_out: i128,
        reserve_in: i128,
        reserve_out: i128,
        fee_bps: u32,
    ) -> i128 {
        let numerator = reserve_in * amount_out * 10_000;
        let denominator = (reserve_out - amount_out) * (10_000 - fee_bps as i128);
        // Ceiling division: the router overfunds exact-out swaps by at most one unit.
        (numerator + denominator - 1) / denominator
    }

    /// Returns the canonical (sorted) order of two tokens, matching how the
    /// factory keys pair storage.
    fn sorted(t0: &Address, t1: &Address) -> (Address, Address) {
        if t0 < t1 {
            (t0.clone(), t1.clone())
        } else {
            (t1.clone(), t0.clone())
        }
    }

    #[test]
    fn test_multi_hop_exact_in_leaves_no_residual_on_router() {
        // tokens[0] = token_a, tokens[1] = hub, tokens[2] = token_b
        let (env, _factory, router, tokens) =
            deploy_with_router_and_pairs(3, &[(0, 1), (1, 2)], 1_000_000);
        let user = Address::generate(&env);

        let token_a = tokens.first().unwrap().clone();
        let hub = tokens.get(1).unwrap().clone();
        let token_b = tokens.get(2).unwrap().clone();

        let token_a_client = TokenClient::new(&env, &token_a);
        let token_b_client = TokenClient::new(&env, &token_b);
        let hub_client = TokenClient::new(&env, &hub);

        // Mint swap input to the user.
        let amount_in = 100_000_i128;
        StellarAssetClient::new(&env, &token_a).mint(&user, &amount_in);

        // 2-hop exact-in: token_a → hub → token_b.
        let mut path: Vec<Address> = Vec::new(&env);
        path.push_back(token_a.clone());
        path.push_back(hub.clone());
        path.push_back(token_b.clone());

        let deadline = env.ledger().timestamp() + 100;
        let amounts = RouterClient::new(&env, &router)
            .swap_exact_tokens_for_tokens(&amount_in, &0, &path, &user, &deadline, &None);
        // swap_exact_tokens_for_tokens returns only the final hop output.
        assert_eq!(amounts.len(), 1, "the wrapper returns only the final amount");
        assert!(amounts.get(0).unwrap() > 0, "final output must be positive");

        // The user received exactly the quoted final output…
        assert_eq!(
            token_b_client.balance(&user),
            amounts.get(0).unwrap(),
            "user must receive the quoted final output"
        );

        // …and no token of any kind is stranded on the router (issue #352).
        assert_eq!(
            token_a_client.balance(&router),
            0,
            "no input-token residual may remain on the router"
        );
        assert_eq!(
            hub_client.balance(&router),
            0,
            "no intermediate-token residual may remain on the router"
        );
        assert_eq!(
            token_b_client.balance(&router),
            0,
            "no output-token residual may remain on the router"
        );

        // Hops actually consumed the input: pair(A, hub) now holds amount_in of token_a.
        let factory_client = FactoryClient::new(&env, &_factory);
        let (s0, s1) = sorted(&token_a, &hub);
        let pair_ah = factory_client.get_pair(&s0, &s1).unwrap();
        assert_eq!(
            token_a_client.balance(&pair_ah),
            1_000_000 + amount_in,
            "first pair must have received the full input amount"
        );
    }

    #[test]
    fn test_three_hop_exact_in_leaves_no_residual_on_router() {
        // tokens[0] = token_a, tokens[1] = hub_1, tokens[2] = hub_2, tokens[3] = token_b
        let (env, _factory, router, tokens) =
            deploy_with_router_and_pairs(4, &[(0, 1), (1, 2), (2, 3)], 1_000_000);
        let user = Address::generate(&env);

        let token_a = tokens.first().unwrap().clone();
        let hub_1 = tokens.get(1).unwrap().clone();
        let hub_2 = tokens.get(2).unwrap().clone();
        let token_b = tokens.get(3).unwrap().clone();

        let token_a_client = TokenClient::new(&env, &token_a);
        let token_b_client = TokenClient::new(&env, &token_b);
        let hub_1_client = TokenClient::new(&env, &hub_1);
        let hub_2_client = TokenClient::new(&env, &hub_2);

        let amount_in = 50_000_i128;
        StellarAssetClient::new(&env, &token_a).mint(&user, &amount_in);

        let mut path: Vec<Address> = Vec::new(&env);
        path.push_back(token_a.clone());
        path.push_back(hub_1.clone());
        path.push_back(hub_2.clone());
        path.push_back(token_b.clone());

        let deadline = env.ledger().timestamp() + 100;
        let amounts = RouterClient::new(&env, &router)
            .swap_exact_tokens_for_tokens(&amount_in, &0, &path, &user, &deadline, &None);
        // swap_exact_tokens_for_tokens returns only the final hop output.
        assert_eq!(amounts.len(), 1, "the wrapper returns only the final amount");

        assert_eq!(
            token_b_client.balance(&user),
            amounts.get(0).unwrap(),
            "user must receive exactly the final hop output"
        );
        // The router forwarded every intermediate amount and kept nothing.
        assert_eq!(
            token_a_client.balance(&router),
            0,
            "input-token dust must be swept to the user"
        );
        assert_eq!(hub_1_client.balance(&router), 0);
        assert_eq!(hub_2_client.balance(&router), 0);
        assert_eq!(token_b_client.balance(&router), 0);
    }

    #[test]
    fn test_forward_dust_is_swept_to_recipient() {
        // tokens[0] = token_a, tokens[1] = token_b
        let (env, _factory, router, tokens) = deploy_with_router_and_pairs(2, &[(0, 1)], 1_000_000);
        let user = Address::generate(&env);

        let token_a = tokens.first().unwrap().clone();
        let token_b = tokens.get(1).unwrap().clone();

        let token_a_client = TokenClient::new(&env, &token_a);
        let token_b_client = TokenClient::new(&env, &token_b);

        // Simulate dust stranded on the router by a previously reverted path.
        let stranded_dust = 7_i128;
        StellarAssetClient::new(&env, &token_a).mint(&router, &stranded_dust);

        let amount_in = 10_000_i128;
        StellarAssetClient::new(&env, &token_a).mint(&user, &amount_in);

        let mut path: Vec<Address> = Vec::new(&env);
        path.push_back(token_a.clone());
        path.push_back(token_b.clone());

        let deadline = env.ledger().timestamp() + 100;
        let amounts = RouterClient::new(&env, &router)
            .swap_exact_tokens_for_tokens(&amount_in, &0, &path, &user, &deadline, &None);

        assert_eq!(amounts.len(), 1);
        assert_eq!(
            token_b_client.balance(&user),
            amounts.get(0).unwrap(),
            "user must receive the quoted output"
        );
        // The stranded dust was swept to the recipient together with the swap.
        assert_eq!(
            token_a_client.balance(&user),
            stranded_dust,
            "pre-existing router dust must be refunded to the recipient"
        );
        assert_eq!(
            token_a_client.balance(&router),
            0,
            "router must hold zero input token after the swap"
        );
        assert_eq!(token_b_client.balance(&router), 0);
    }

    #[test]
    fn test_swap_exact_out_leaves_no_residual_on_router() {
        // tokens[0] = token_a, tokens[1] = token_b
        let (env, factory, router, tokens) = deploy_with_router_and_pairs(2, &[(0, 1)], 1_000_000);
        let user = Address::generate(&env);

        let token_a = tokens.first().unwrap().clone();
        let token_b = tokens.get(1).unwrap().clone();

        let token_a_client = TokenClient::new(&env, &token_a);
        let token_b_client = TokenClient::new(&env, &token_b);

        let factory_client = FactoryClient::new(&env, &factory);
        let (s0, s1) = sorted(&token_a, &token_b);
        let pair_address = factory_client.get_pair(&s0, &s1).unwrap();
        let pair_client = PairClient::new(&env, &pair_address);
        let (reserve_a, reserve_b, _) = pair_client.get_reserves();
        let fee_bps = pair_client.get_current_fee_bps();

        let amount_out = 1_000_i128;
        let (r_in, r_out) =
            if token_a < token_b { (reserve_a, reserve_b) } else { (reserve_b, reserve_a) };
        let expected_in = compute_amount_in(amount_out, r_in, r_out, fee_bps);

        // add_liquidity above consumed the user's full seeded balance; mint
        // fresh input so the router can pull the exact required amount.
        StellarAssetClient::new(&env, &token_a).mint(&user, &(expected_in * 100));

        let mut path: Vec<Address> = Vec::new(&env);
        path.push_back(token_a.clone());
        path.push_back(token_b.clone());

        let deadline = env.ledger().timestamp() + 100;
        let user_before = token_a_client.balance(&user);
        let amounts = RouterClient::new(&env, &router).swap_tokens_for_exact_tokens(
            &amount_out,
            &(&expected_in * 2),
            &path,
            &user,
            &deadline,
            &None,
        );

        assert_eq!(amounts.len(), 1);
        assert_eq!(
            amounts.get(0).unwrap(),
            expected_in,
            "exact-out must charge exactly the computed input"
        );
        assert_eq!(
            token_a_client.balance(&user),
            user_before - expected_in,
            "user must be charged exactly the required input"
        );
        assert_eq!(token_b_client.balance(&user), amount_out);
        // No input-token residual may remain stuck on the router (issue #352).
        assert_eq!(token_a_client.balance(&router), 0);
        assert_eq!(token_b_client.balance(&router), 0);
    }
}
