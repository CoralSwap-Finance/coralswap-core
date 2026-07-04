use soroban_sdk::{
    contract, contractclient, contractimpl, contracttype,
    testutils::{Address as _, Ledger as _},
    xdr::ToXdr,
    Address, Bytes, BytesN, Env, Vec,
};

// ---------------------------------------------------------------------------
// MockFactory
// ---------------------------------------------------------------------------

#[contract]
pub struct MockFactory;

#[contracttype]
#[derive(Clone)]
pub enum MFKey {
    Pair(Address, Address),
}

#[contractimpl]
impl MockFactory {
    pub fn set_pair(env: Env, token_a: Address, token_b: Address, pair: Address) {
        let (t0, t1) = if token_a < token_b { (token_a, token_b) } else { (token_b, token_a) };
        env.storage().instance().set(&MFKey::Pair(t0, t1), &pair);
    }

    pub fn get_pair(env: Env, token_a: Address, token_b: Address) -> Option<Address> {
        let (t0, t1) = if token_a < token_b { (token_a, token_b) } else { (token_b, token_a) };
        env.storage().instance().get(&MFKey::Pair(t0, t1))
    }

    pub fn create_pair(_env: Env, _token_a: Address, _token_b: Address) -> Address {
        panic!("not needed for router unit tests")
    }
}

// ---------------------------------------------------------------------------
// MockPair
// ---------------------------------------------------------------------------

#[contract]
pub struct MockPair;

#[contracttype]
#[derive(Clone)]
pub enum MPKey {
    ReserveA,
    ReserveB,
    BurnAmountA,
    BurnAmountB,
    LiquidityToMint,
}

#[contractimpl]
impl MockPair {
    pub fn set_reserves(env: Env, reserve_a: i128, reserve_b: i128) {
        env.storage().instance().set(&MPKey::ReserveA, &reserve_a);
        env.storage().instance().set(&MPKey::ReserveB, &reserve_b);
    }

    pub fn get_reserves(env: Env) -> (i128, i128, u64) {
        let a: i128 = env.storage().instance().get(&MPKey::ReserveA).unwrap_or(0);
        let b: i128 = env.storage().instance().get(&MPKey::ReserveB).unwrap_or(0);
        (a, b, 0)
    }

    pub fn set_burn_amounts(env: Env, amount_a: i128, amount_b: i128) {
        env.storage().instance().set(&MPKey::BurnAmountA, &amount_a);
        env.storage().instance().set(&MPKey::BurnAmountB, &amount_b);
    }

    pub fn burn(env: Env, _to: Address) -> (i128, i128) {
        let a: i128 = env.storage().instance().get(&MPKey::BurnAmountA).unwrap_or(0);
        let b: i128 = env.storage().instance().get(&MPKey::BurnAmountB).unwrap_or(0);
        (a, b)
    }

    pub fn set_liquidity_to_mint(env: Env, liquidity: i128) {
        env.storage().instance().set(&MPKey::LiquidityToMint, &liquidity);
    }

    pub fn mint(env: Env, _to: Address) -> i128 {
        env.storage().instance().get(&MPKey::LiquidityToMint).unwrap_or(0)
    }

    pub fn swap(_env: Env, _amount_a_out: i128, _amount_b_out: i128, _to: Address) {}

    pub fn lp_token(_env: Env) -> Address {
        panic!("not needed for router unit tests")
    }

    pub fn get_current_fee_bps(_env: Env) -> u32 {
        30
    }
}

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

use crate::Router;

fn deploy_router(env: &Env) -> (Address, Address) {
    let router_id = env.register_contract(None, Router);
    let factory_id = env.register_contract(None, MockFactory);
    let router = RouterClient::new(env, &router_id);
    router.initialize(&factory_id, &Vec::new(env));
    (router_id, factory_id)
}

fn generate_tokens(env: &Env, n: u32) -> Vec<Address> {
    let mut tokens: Vec<Address> = Vec::new(env);
    for _ in 0..n {
        tokens.push_back(Address::generate(env));
    }
    tokens
}

fn setup_pair(
    env: &Env,
    factory_id: &Address,
    token_a: &Address,
    token_b: &Address,
    reserve_a: i128,
    reserve_b: i128,
) -> Address {
    let pair_id = env.register_contract(None, MockPair);
    let pair = MockPairClient::new(env, &pair_id);
    pair.set_reserves(&reserve_a, &reserve_b);

    let factory = MockFactoryClient::new(env, factory_id);
    factory.set_pair(token_a, token_b, &pair_id);
    pair_id
}

fn make_path(env: &Env, tokens: &Vec<Address>) -> Vec<Address> {
    let mut path: Vec<Address> = Vec::new(env);
    for i in 0..tokens.len() {
        path.push_back(tokens.get(i).unwrap());
    }
    path
}

// ---------------------------------------------------------------------------
// RouterClient helper
// ---------------------------------------------------------------------------

#[contractclient(name = "RouterClient")]
#[allow(dead_code)]
pub trait RouterInterface {
    fn initialize(env: Env, factory: Address, hubs: Vec<Address>);
    fn set_hubs(env: Env, hubs: Vec<Address>);
    fn get_hubs(env: Env) -> Vec<Address>;
    fn get_best_path(
        env: Env,
        token_in: Address,
        token_out: Address,
        amount_in: i128,
    ) -> (Vec<Address>, i128);
    fn swap_exact_tokens_multi_hop(
        env: Env,
        path: Vec<Address>,
        amount_in: i128,
        amount_out_min: i128,
        to: Address,
        deadline: u64,
    ) -> i128;
    fn swap_exact_tokens_for_tokens(
        env: Env,
        amount_in: i128,
        amount_out_min: i128,
        path: Vec<Address>,
        to: Address,
        deadline: u64,
    ) -> Vec<i128>;
    fn swap_tokens_for_exact_tokens(
        env: Env,
        amount_out: i128,
        amount_in_max: i128,
        path: Vec<Address>,
        to: Address,
        deadline: u64,
    ) -> Vec<i128>;
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
    fn commit_swap(env: Env, sender: Address, hash: BytesN<32>);
    fn reveal_swap(
        env: Env,
        sender: Address,
        token_in: Address,
        token_out: Address,
        amount_in: i128,
        min_out: i128,
        nonce: u64,
        salt: BytesN<32>,
    ) -> i128;
}

mod helpers_test;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn test_contract_compiles() {
    // Contract compiles and links correctly
}

// ===================== get_best_path =====================

#[test]
fn test_get_best_path_identical_tokens() {
    let env = Env::default();
    let (router_id, _factory_id) = deploy_router(&env);
    let router = RouterClient::new(&env, &router_id);
    let token = Address::generate(&env);

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        router.get_best_path(&token, &token, &1000);
    }));
    assert!(result.is_err(), "identical tokens must fail");
}

#[test]
fn test_get_best_path_zero_amount() {
    let env = Env::default();
    let (router_id, _factory_id) = deploy_router(&env);
    let router = RouterClient::new(&env, &router_id);
    let a = Address::generate(&env);
    let b = Address::generate(&env);

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        router.get_best_path(&a, &b, &0);
    }));
    assert!(result.is_err(), "zero amount must fail");
}

#[test]
fn test_get_best_path_no_factory_set() {
    let env = Env::default();
    let router_id = env.register_contract(None, Router);
    let router = RouterClient::new(&env, &router_id);
    let a = Address::generate(&env);
    let b = Address::generate(&env);

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        router.get_best_path(&a, &b, &1000);
    }));
    assert!(result.is_err(), "no factory must fail");
}

#[test]
fn test_get_best_path_direct_pair() {
    let env = Env::default();
    let (router_id, factory_id) = deploy_router(&env);
    let router = RouterClient::new(&env, &router_id);
    let tokens = generate_tokens(&env, 2);
    let token_a = tokens.get(0).unwrap();
    let token_b = tokens.get(1).unwrap();

    setup_pair(&env, &factory_id, &token_a, &token_b, 100_000, 100_000);

    let (path, expected_out) = router.get_best_path(&token_a, &token_b, &1000);
    assert_eq!(path.len(), 2, "direct path must have 2 entries");
    assert_eq!(path.get(0).unwrap(), token_a);
    assert_eq!(path.get(1).unwrap(), token_b);
    assert!(expected_out > 0, "expected output must be positive");
}

#[test]
fn test_get_best_path_two_hop() {
    let env = Env::default();
    let (router_id, factory_id) = deploy_router(&env);
    let router = RouterClient::new(&env, &router_id);
    let tokens = generate_tokens(&env, 3);
    let token_a = tokens.get(0).unwrap();
    let token_b = tokens.get(1).unwrap();
    let hub = tokens.get(2).unwrap();

    let mut hubs: Vec<Address> = Vec::new(&env);
    hubs.push_back(hub.clone());
    router.set_hubs(&hubs);

    setup_pair(&env, &factory_id, &token_a, &hub, 100_000, 100_000);
    setup_pair(&env, &factory_id, &hub, &token_b, 200_000, 200_000);

    let (path, expected_out) = router.get_best_path(&token_a, &token_b, &1000);
    assert_eq!(path.len(), 3, "2-hop path must have 3 entries");
    assert_eq!(path.get(0).unwrap(), token_a);
    assert_eq!(path.get(1).unwrap(), hub);
    assert_eq!(path.get(2).unwrap(), token_b);
    assert!(expected_out > 0, "expected output must be positive");
}

#[test]
fn test_get_best_path_prefers_highest_output() {
    let env = Env::default();
    let (router_id, factory_id) = deploy_router(&env);
    let router = RouterClient::new(&env, &router_id);
    let tokens = generate_tokens(&env, 4);
    let token_a = tokens.get(0).unwrap();
    let token_b = tokens.get(1).unwrap();
    let hub1 = tokens.get(2).unwrap();
    let hub2 = tokens.get(3).unwrap();

    let mut hubs: Vec<Address> = Vec::new(&env);
    hubs.push_back(hub1.clone());
    hubs.push_back(hub2.clone());
    router.set_hubs(&hubs);

    // Direct pair with very low liquidity → low output
    setup_pair(&env, &factory_id, &token_a, &token_b, 500, 500);

    // hub1 route with high liquidity
    setup_pair(&env, &factory_id, &token_a, &hub1, 100_000, 100_000);
    setup_pair(&env, &factory_id, &hub1, &token_b, 100_000, 100_000);

    // hub2 route with low liquidity
    setup_pair(&env, &factory_id, &token_a, &hub2, 1_000, 1_000);
    setup_pair(&env, &factory_id, &hub2, &token_b, 1_000, 1_000);

    let (path, expected_out) = router.get_best_path(&token_a, &token_b, &1000);
    assert_eq!(path.len(), 3, "should select 2-hop via best hub");
    assert_eq!(path.get(1).unwrap(), hub1, "should prefer higher-liquidity hub");
    assert!(expected_out > 0);
}

#[test]
fn test_get_best_path_three_hop() {
    let env = Env::default();
    let (router_id, factory_id) = deploy_router(&env);
    let router = RouterClient::new(&env, &router_id);
    let tokens = generate_tokens(&env, 4);
    let token_a = tokens.get(0).unwrap();
    let token_b = tokens.get(1).unwrap();
    let hub1 = tokens.get(2).unwrap();
    let hub2 = tokens.get(3).unwrap();

    let mut hubs: Vec<Address> = Vec::new(&env);
    hubs.push_back(hub1.clone());
    hubs.push_back(hub2.clone());
    router.set_hubs(&hubs);

    setup_pair(&env, &factory_id, &token_a, &hub1, 100_000, 100_000);
    setup_pair(&env, &factory_id, &hub1, &hub2, 100_000, 100_000);
    setup_pair(&env, &factory_id, &hub2, &token_b, 100_000, 100_000);

    let (path, expected_out) = router.get_best_path(&token_a, &token_b, &1000);
    assert_eq!(path.len(), 4, "3-hop path must have 4 entries");
    assert_eq!(path.get(0).unwrap(), token_a);
    assert_eq!(path.get(1).unwrap(), hub1);
    assert_eq!(path.get(2).unwrap(), hub2);
    assert_eq!(path.get(3).unwrap(), token_b);
    assert!(expected_out > 0);
}

#[test]
fn test_get_best_path_no_route() {
    let env = Env::default();
    let (router_id, _factory_id) = deploy_router(&env);
    let router = RouterClient::new(&env, &router_id);
    let tokens = generate_tokens(&env, 2);
    let token_a = tokens.get(0).unwrap();
    let token_b = tokens.get(1).unwrap();

    // Set up a hub but no pairs connecting token_a or token_b
    let mut hubs: Vec<Address> = Vec::new(&env);
    hubs.push_back(Address::generate(&env));
    router.set_hubs(&hubs);

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        router.get_best_path(&token_a, &token_b, &1000);
    }));
    assert!(result.is_err(), "no feasible route must fail");
}

// ===================== swap_exact_tokens_multi_hop =====================

#[test]
fn test_swap_multi_hop_expired_deadline() {
    let env = Env::default();
    let (router_id, _factory_id) = deploy_router(&env);
    let router = RouterClient::new(&env, &router_id);
    let tokens = generate_tokens(&env, 2);
    let path = make_path(&env, &tokens);

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        router.swap_exact_tokens_multi_hop(
            &path,
            &1000,
            &1,
            &Address::generate(&env),
            &1, // deadline in the past (ledger timestamp is 2000)
        );
    }));
    assert!(result.is_err(), "expired deadline must fail");
}

#[test]
fn test_swap_multi_hop_zero_amount() {
    let env = Env::default();
    let (router_id, _factory_id) = deploy_router(&env);
    let router = RouterClient::new(&env, &router_id);
    let tokens = generate_tokens(&env, 2);
    let path = make_path(&env, &tokens);

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        router.swap_exact_tokens_multi_hop(&path, &0, &1, &Address::generate(&env), &u64::MAX);
    }));
    assert!(result.is_err(), "zero amount must fail");
}

#[test]
fn test_swap_multi_hop_invalid_path_too_short() {
    let env = Env::default();
    let (router_id, _factory_id) = deploy_router(&env);
    let router = RouterClient::new(&env, &router_id);
    let mut path: Vec<Address> = Vec::new(&env);
    path.push_back(Address::generate(&env));

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        router.swap_exact_tokens_multi_hop(&path, &1000, &1, &Address::generate(&env), &u64::MAX);
    }));
    assert!(result.is_err(), "too-short path must fail");
}

#[test]
fn test_swap_multi_hop_invalid_path_too_long() {
    let env = Env::default();
    let (router_id, _factory_id) = deploy_router(&env);
    let router = RouterClient::new(&env, &router_id);
    let tokens = generate_tokens(&env, 5);
    let path = make_path(&env, &tokens);

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        router.swap_exact_tokens_multi_hop(&path, &1000, &1, &Address::generate(&env), &u64::MAX);
    }));
    assert!(result.is_err(), "too-long path (4+ hops) must fail");
}

// ===================== swap_tokens_for_exact_tokens =====================

#[test]
fn test_swap_exact_out_expired_deadline() {
    let env = Env::default();
    let (router_id, _factory_id) = deploy_router(&env);
    let router = RouterClient::new(&env, &router_id);
    let tokens = generate_tokens(&env, 2);
    let path = make_path(&env, &tokens);

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        router.swap_tokens_for_exact_tokens(&100, &1000, &path, &Address::generate(&env), &1);
    }));
    assert!(result.is_err(), "expired deadline must fail");
}

#[test]
fn test_swap_exact_out_zero_amount() {
    let env = Env::default();
    let (router_id, _factory_id) = deploy_router(&env);
    let router = RouterClient::new(&env, &router_id);
    let tokens = generate_tokens(&env, 2);
    let path = make_path(&env, &tokens);

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        router.swap_tokens_for_exact_tokens(&0, &1000, &path, &Address::generate(&env), &u64::MAX);
    }));
    assert!(result.is_err(), "zero output amount must fail");
}

#[test]
fn test_swap_exact_out_invalid_path() {
    let env = Env::default();
    let (router_id, _factory_id) = deploy_router(&env);
    let router = RouterClient::new(&env, &router_id);
    let mut path: Vec<Address> = Vec::new(&env);
    path.push_back(Address::generate(&env));

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        router.swap_tokens_for_exact_tokens(
            &100,
            &1000,
            &path,
            &Address::generate(&env),
            &u64::MAX,
        );
    }));
    assert!(result.is_err(), "too-short path must fail");
}

// ===========================================================================
// MockToken — minimal SEP-41 token stub for commit-reveal lifecycle tests
// ===========================================================================

#[contract]
pub struct MockToken;

#[contractimpl]
impl MockToken {
    pub fn transfer(_env: Env, _from: Address, _to: Address, _amount: i128) {}
    pub fn balance(_env: Env, _id: Address) -> i128 {
        i128::MAX
    }
    pub fn allowance(_env: Env, _from: Address, _spender: Address) -> i128 {
        0
    }
    pub fn approve(
        _env: Env,
        _from: Address,
        _spender: Address,
        _amount: i128,
        _expiration_ledger: u32,
    ) {
    }
    pub fn transfer_from(
        _env: Env,
        _spender: Address,
        _from: Address,
        _to: Address,
        _amount: i128,
    ) {
    }
    pub fn decimals(_env: Env) -> u32 {
        7
    }
    pub fn name(env: Env) -> soroban_sdk::String {
        soroban_sdk::String::from_str(&env, "Mock")
    }
    pub fn symbol(env: Env) -> soroban_sdk::String {
        soroban_sdk::String::from_str(&env, "MCK")
    }
}

// ---------------------------------------------------------------------------
// Commit-reveal test helpers
// ---------------------------------------------------------------------------

/// Replicates the on-chain hash so tests can build valid commitments.
fn make_commit_hash(
    env: &Env,
    sender: &Address,
    token_in: &Address,
    token_out: &Address,
    amount_in: i128,
    min_out: i128,
    nonce: u64,
    salt: &BytesN<32>,
) -> BytesN<32> {
    let mut data = Bytes::new(env);
    data.append(&sender.to_xdr(env));
    data.append(&token_in.to_xdr(env));
    data.append(&token_out.to_xdr(env));
    data.append(&Bytes::from_slice(env, &amount_in.to_be_bytes()));
    data.append(&Bytes::from_slice(env, &min_out.to_be_bytes()));
    data.append(&Bytes::from_slice(env, &nonce.to_be_bytes()));
    let salt_bytes: Bytes = salt.clone().into();
    data.append(&salt_bytes);
    env.crypto().sha256(&data).into()
}

/// Deploys a router + factory + pair + two mock tokens ready for a 1-hop swap.
fn deploy_router_with_pair(env: &Env) -> (Address, Address, Address, Address, Address) {
    let (router_id, factory_id) = deploy_router(env);

    let token_in_id = env.register_contract(None, MockToken);
    let token_out_id = env.register_contract(None, MockToken);

    let pair_id = setup_pair(env, &factory_id, &token_in_id, &token_out_id, 1_000_000, 1_000_000);

    // Give MockPair a non-zero fee so get_best_path succeeds
    let pair = MockPairClient::new(env, &pair_id);
    pair.set_reserves(&1_000_000, &1_000_000);

    (router_id, factory_id, token_in_id, token_out_id, pair_id)
}

// ===========================================================================
// ===================== commit_swap / reveal_swap tests =====================
// ===========================================================================

#[test]
fn test_commit_swap_stores_entry() {
    let env = Env::default();
    env.mock_all_auths();
    let (router_id, _) = deploy_router(&env);
    let router = RouterClient::new(&env, &router_id);

    let sender = Address::generate(&env);
    let token_in = Address::generate(&env);
    let token_out = Address::generate(&env);
    let salt = BytesN::from_array(&env, &[7u8; 32]);
    let nonce: u64 = 1;

    let hash = make_commit_hash(&env, &sender, &token_in, &token_out, 1000, 0, nonce, &salt);

    // Should store without error
    router.commit_swap(&sender, &hash);
}

#[test]
fn test_reveal_without_commit_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let (router_id, _) = deploy_router(&env);
    let router = RouterClient::new(&env, &router_id);

    let sender = Address::generate(&env);
    let token_in = Address::generate(&env);
    let token_out = Address::generate(&env);
    let salt = BytesN::from_array(&env, &[1u8; 32]);

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        router.reveal_swap(&sender, &token_in, &token_out, &1000, &0, &1_u64, &salt);
    }));
    assert!(result.is_err(), "reveal without prior commit must fail (CommitNotFound)");
}

#[test]
fn test_reveal_same_ledger_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let (router_id, _) = deploy_router(&env);
    let router = RouterClient::new(&env, &router_id);

    let sender = Address::generate(&env);
    let token_in = Address::generate(&env);
    let token_out = Address::generate(&env);
    let salt = BytesN::from_array(&env, &[2u8; 32]);
    let nonce: u64 = 1;

    let hash = make_commit_hash(&env, &sender, &token_in, &token_out, 1000, 0, nonce, &salt);
    router.commit_swap(&sender, &hash);

    // No ledger advance — same sequence number
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        router.reveal_swap(&sender, &token_in, &token_out, &1000, &0, &nonce, &salt);
    }));
    assert!(result.is_err(), "reveal on same ledger must fail (CommitRevealTooEarly)");
}

#[test]
fn test_reveal_wrong_hash_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let (router_id, _) = deploy_router(&env);
    let router = RouterClient::new(&env, &router_id);

    let sender = Address::generate(&env);
    let token_in = Address::generate(&env);
    let token_out = Address::generate(&env);
    let salt = BytesN::from_array(&env, &[3u8; 32]);
    let nonce: u64 = 1;

    let hash = make_commit_hash(&env, &sender, &token_in, &token_out, 1000, 0, nonce, &salt);
    router.commit_swap(&sender, &hash);

    // Advance one ledger
    env.ledger().set_sequence_number(env.ledger().sequence() + 1);

    // Reveal with a different amount — hash won't match
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        router.reveal_swap(&sender, &token_in, &token_out, &9999, &0, &nonce, &salt);
    }));
    assert!(result.is_err(), "reveal with wrong params must fail (CommitHashMismatch)");
}

#[test]
fn test_reveal_nonce_replay_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let (router_id, _) = deploy_router(&env);
    let router = RouterClient::new(&env, &router_id);

    let sender = Address::generate(&env);
    let nonce: u64 = 42;

    // Mark nonce as already used directly via contract storage
    env.as_contract(&router_id, || {
        crate::storage::set_nonce_used(&env, &sender, nonce);
    });

    let token_in = Address::generate(&env);
    let token_out = Address::generate(&env);
    let salt = BytesN::from_array(&env, &[4u8; 32]);
    let hash = make_commit_hash(&env, &sender, &token_in, &token_out, 1000, 0, nonce, &salt);
    router.commit_swap(&sender, &hash);

    env.ledger().set_sequence_number(env.ledger().sequence() + 1);

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        router.reveal_swap(&sender, &token_in, &token_out, &1000, &0, &nonce, &salt);
    }));
    assert!(result.is_err(), "reused nonce must fail (NonceAlreadyUsed)");
}

#[test]
fn test_commit_reveal_full_lifecycle() {
    let env = Env::default();
    env.mock_all_auths();

    let (router_id, _factory_id, token_in_id, token_out_id, _pair_id) =
        deploy_router_with_pair(&env);
    let router = RouterClient::new(&env, &router_id);

    let sender = Address::generate(&env);
    let amount_in: i128 = 1_000;
    let min_out: i128 = 0;
    let nonce: u64 = 7;
    let salt = BytesN::from_array(&env, &[0xABu8; 32]);

    let hash = make_commit_hash(
        &env,
        &sender,
        &token_in_id,
        &token_out_id,
        amount_in,
        min_out,
        nonce,
        &salt,
    );

    // Step 1: commit
    router.commit_swap(&sender, &hash);

    // Step 2: advance one ledger (minimum delay)
    env.ledger().set_sequence_number(env.ledger().sequence() + 1);

    // Step 3: reveal — hash validates, nonce is consumed, swap executes
    let out = router.reveal_swap(
        &sender,
        &token_in_id,
        &token_out_id,
        &amount_in,
        &min_out,
        &nonce,
        &salt,
    );
    assert!(out > 0, "revealed swap must return positive output");

    // Step 4: same nonce is now rejected
    let hash2 = make_commit_hash(
        &env,
        &sender,
        &token_in_id,
        &token_out_id,
        amount_in,
        min_out,
        nonce,
        &salt,
    );
    router.commit_swap(&sender, &hash2);
    env.ledger().set_sequence_number(env.ledger().sequence() + 1);

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        router.reveal_swap(
            &sender,
            &token_in_id,
            &token_out_id,
            &amount_in,
            &min_out,
            &nonce,
            &salt,
        );
    }));
    assert!(result.is_err(), "replayed nonce must be rejected");
}
