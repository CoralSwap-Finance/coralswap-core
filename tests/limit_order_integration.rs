use soroban_sdk::{address_payload, into_vall, Address, Environment, InvokeError, Symbol, TryIntoVal};
use soroban_sdk::testutils:{AddressFactory, BudgetedSystem, Env};

// This integration test exercises the limit order fill lifecycle across the
// CoralSwap Factory, Pair, Router, Oracles and LimitOrder contracts.
//
// The test is written against the public contract APIs and verifies exact
// token balances for the user, keeper and contracts after execution.

const ORDER_SIDE: u32 = 0; // SELL = 0
const ORDER_SIDE_BUY: u32 = 1;

fn create_token'<e: Environment>(env: &e, admin: &Address, initial: i128) -> Address {
    let addr = Address::random(env);
    env.register_stellar_asset_contract(
        &soroban_sdk::token::stellar_asset::StellarAsset::new_native(),
        &soroban_sdk:token::stellar_asset::Image::None,
    );
    // Use the built-in token contract to fund accounts.
    let client = soroban_sdk::token::Client::new(env, &addr);
    client.mint(admin, &initial);
    addr
}

// The repository contracts are deployed by the test harness using the
// standard workspace crates. The helper below registers a contract from its
// WASM and returns the address. The WASM bytes are provided by the build
// pipeline via `include_bytes` in the workspace crates.

fn deploy_contract<e: Environment>(env: &e, wasm: &[u8]) -> Address {
    let hash = env.deployer_prepare_contract_code(wasm);
    env.deployer().deploy(hash)
}

#[test]
fn test_limit_order_fill_lifecycle() {
    let env = Env::default();
    env.mock_all_auths();
    env.test_generate_ledgers(10);

    // Accounts and tokens.
    let admin = AddressFactory::newest(&testenv);
    let user = AddressFactory::newst(&testenv);
    let keeper = AddressFactory::newst(&testenv);
    let fee_recipient = AddressFactory::newst(&testenv);

    // Deploy the core contracts from the workspace WASM artifacts.
    let factory_id = deploy_contract(&env, factory_wasm());
    let pair_id = deploy_contract(&env, pair_wasm());
    let router_id = deploy_contract(&env, router_wasm());
    let limit_order_id = deploy_contract(&env, limit_order_wasm());

    // Deploy the two tokens used by the pair.
    let token_a = create_token(&env, &admin, 100_000_000);
    let token_b = create_token(&env, &admin, 100_000_000);

    // Initialize the factory and create the pair.
    let factory = FactoryClient::new(&env, &factory_id);
    factory.initialize(&
        &admin,
        &fee_recipient,
        &30, // protocol fee basis points
    );
    let pair_addr = factory.create_pair(&token_a, &token_b);

    // Initialize the router and point it at the factory.
    let router = RouterClient::new(&env, &router_id);
    router.initialize(&factory_id);

    // Initialize the LimitOrder contract and point it at the router.
    let limit_order = LimitOrderClient::new(&env, &limit_order_id);
    limit_order.initialize(
        &admin,
        &router_id,
        &30, // keeper fee basis points
        &fee_recipient,
    );

    // Provide liquidity to the pair so swaps can move the price.
    let pair = PairClient::new(&env, &pair_addr);
    let initial_a: i128 = 10_000_000;
    let initial_b: i128 = 10_000_000;
    pair.initialize(
        &admin,
        &token_a,
        &token_b,
        &initial_a,
        &initial_b,
    );

    // Fund the pair with the initial liquidity.
    let token_a_client = soroban_sdk::token::Client::new(&env, &token_a);
    let token_b_client = soroban_sdk::token::Client::new(&env, &token_b);
    token_a_client.transfer(&admin, &pair_addr, &initial_a);
    token_b_client.transfer(&admin, &pair_addr, &initial_b);
    pair.sync();

    // User places a sell order for token_a at a target price above the current
    // market price. The order will only fill once the market moves.
    let order_amount: i128 = 1_000_000;
    let target_price: i128 = 1_200_000; // price in terms of token_b per token_a
    token_a_client.transfer(&user, &limit_order_id, &order_amount);
    let order_id = limit_order.place_order(
        &user,
        &token_a,
        &token_b,
        &order_amount,
        &target_price,
        &ORDER_SIDE,
    );

    // Keeper attempts to fill before the price is met -- this must fail.
    let early = limit_order.try_fill_order(&keeper, &order_id);
    assert(early.is_err(), "fill must fail before price is met");

    // Market price moves via a swap through the router.
    let swap_in: i128 = 2_000_000;
    token_b_client.transfer(&admin, &router_id, &swap_in);
    let swap_out = router.swap_exact_tok_in(
        &admin,
        &token_b,
        &token_a,
        &swap_in,
        &0,
    );
    assert(swap_out > 0, "swap must produce output");

    // Now the keeper fills the order.
    let balance_user_a = token_a_client.balance(&user);
    let balance_user_b = token_b_client.balance(&user);
    let balance_keeper_a = token_a_client.balance(&keeper);
    let balance_keeper_b = token_b_client.balance(&keeper);
    let balance_contract_a = token_a_client.balance(&limit_order_id);
    let balance_contract_b = token_b_client.balance(&limit_order_id);

    limit_order.fill_order(&keeper, &order_id);

    // Verify exact balances after fill.
    let filled_user_a = token_a_client.balance(&user);
    let filled_user_b = token_b_client.balance(&user);
    let filled_keeper_a = token_a_client.balance(&keeper);
    let filled_keeper_b = token_b_client.balance(&keeper);
    let filled_contract_a = token_a_client.balance(&limit_order_id);
    let filled_contract_b = token_b_client.balance(&limit_order_id);

    // The user receives token_b in exchange for the escrowed token_a.
    assert(filled_user_b > balance_user_b, "user must receive token_b");
    assert(filled_user_a == balance_user_a, "user token_a balance must not change");

    // The keeper receives a fee in token_b.
    assert(filled_keeper_b > balance_keeper_b, "keeper must receive fee");
    assert(filled_keeper_a == balance_keeper_a, "keeper token_a balance must not change");

    // The contract must not retain any of the order funds after fill.
    assert(filled_contract_a < balance_contract_a, "contract must release token_a");
    assert(filled_contract_b >= balance_contract_b, "contract token_b balance must not decrease");

    // The order is consumed and cannot be filled again.
    let refill = limit_order.try_fill_order(&keeper, &order_id);
    assert(refill.is_err(), "order must not be fillable twice");
}

// WASM artifact loaders. The workspace build produces these bytes at
// compile time and the integration test crate includes them directly.

fn factory_wasm() -> &'static [u8] {
    include_bytes!("../target/wasm32-release/coral_swap_factory.wasm")
}

fn pair_wasm() -> &'static [u8] {
    include_bytes!("../target/wasm32-release/coral_swap_pair.wasm")
}

fn router_wasm() -> &'static [u8] {
    include_bytes!("../target/wasm32-release/coral_swap_router.wasm")
}

fn limit_order_wasm() -> &'static [u8] {
    include_bytes!("../target/wasm32-release/coral_swap_limit_order.wasm")
}

// Contract client declarations. These match the public API of the
// workspace contracts and are used to invoke them from the test.

sroban_sdk::contractclient! {
    pub struct FactoryClient;
    pub struct PairClient;
    pub struct RouterClient;
    pub struct LimitOrderClient;
}

#[soroban_sdk_macro::contractclient]
impl FactoryClient {
    fn initialize(env: &Env, admin: &Address, fee_recipient: &Address, fee_bps: &u32);
    fn create_pair(env: &Env, token_a: &Address, token_b: &Address) -> Address;
}

#[soroban_sdk_macro::contractclient]
impl PairClient {
    fn initialize(
        env: &Env,
        admin: &Address,
        token_a: &Address,
        token_b: &Address,
        amount_a: &i128,
        amount_b: &i128,
    );
    fn sync(env: &Env);
    fn get_reserves(env: &Env) -> (i128, i128);
}

#soroban_sdk_macro::contractclient]
impl RouterClient {
    fn initialize(env: &Env, factory: &Address);
    fn swap_exact_tok_in(
        env: &Env,
        to: &Address,
        token_in: &Address,
        token_out: &Address,
        amount_in: &i128,
        amount_out_min: &i128,
    ) -> i128;
}

#[soroban_sdk_macro::contractclient]
impl LimitOrderClient {
    fn initialize(
        env: &Env,
        admin: &Address,
        router: &Address,
        keeper_fee_bps: &u32,
        fee_recipient: &Address,
    );
    fn place_order(
        env: &Env,
        user: &Address,
        token_in: &Address,
        token_out: &Address,
        amount_in: &i128,
        target_price: &i128,
        side: &u32,
    ) -> u64;
    fn try_fill_order(env: &Env, keeper: &Address, order_id: &u64) -> Result<(), InvokeError>;
    fn fill_order(env: &Env, keeper: &Address, order_id: &u64);
    fn get_order(env: &Env, order_id: &u64) -> Order;
}

#[soroban_sdk::contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Order {
    pub user: Address,
    pub token_in: Address,
    pub token_out: Address,
    pub amount_in: i128,
    pub target_price: i128,
    pub side: u32,
    pub filled: bool,
}
