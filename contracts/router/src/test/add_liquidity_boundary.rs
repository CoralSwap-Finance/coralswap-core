//! Property test for `Router::add_liquidity` at the 1-stroop boundary (issue #368).
//!
//! The router quotes deposit amounts with `compute_optimal_amounts`, which
//! floors the ratio-derived side. The pair then re-derives the deposit from its
//! token balances and applies its own independent checks (`MINIMUM_RESERVE`
//! per side, `subsequent_liquidity > 0`, `MIN_LIQUIDITY_FOR_USER`). A quote
//! that a client "simulates" successfully must therefore also execute
//! successfully against the real pair, and vice versa.
//!
//! The matrix runs the real `Pair` and `LpToken` contracts at reserve
//! magnitudes 1e3..1e15 with the slippage floors set exactly to the quoted
//! amounts (the tightest possible min_out), then checks that one stroop above
//! the quote on the ratio-derived side reverts in both simulation and
//! execution.

use coralswap_lp_token::{LpToken, LpTokenClient};
use coralswap_pair::{math::subsequent_liquidity, Pair, PairClient};
use coralswap_shared::{MINIMUM_RESERVE, MIN_LIQUIDITY_FOR_USER};
use soroban_sdk::{
    contract, contractimpl, contracttype,
    testutils::{Address as _, EnvTestConfig},
    Address, Env, String, Vec,
};

use super::{MockFactory, MockFactoryClient, RouterClient};
use crate::{errors::RouterError, helpers::compute_optimal_amounts, Router};

// ── Minimal balance-tracking token ───────────────────────────────────────────

#[contracttype]
enum BoundaryTokenKey {
    Balance(Address),
}

#[contract]
pub struct BoundaryToken;

#[contractimpl]
impl BoundaryToken {
    pub fn mint(env: Env, to: Address, amount: i128) {
        let key = BoundaryTokenKey::Balance(to);
        let bal: i128 = env.storage().persistent().get(&key).unwrap_or(0);
        env.storage().persistent().set(&key, &(bal + amount));
    }

    pub fn transfer(env: Env, from: Address, to: Address, amount: i128) {
        from.require_auth();
        let fk = BoundaryTokenKey::Balance(from);
        let tk = BoundaryTokenKey::Balance(to);
        let fb: i128 = env.storage().persistent().get(&fk).unwrap_or(0);
        if fb < amount {
            panic!("insufficient balance");
        }
        let tb: i128 = env.storage().persistent().get(&tk).unwrap_or(0);
        env.storage().persistent().set(&fk, &(fb - amount));
        env.storage().persistent().set(&tk, &(tb + amount));
    }

    pub fn balance(env: Env, id: Address) -> i128 {
        env.storage().persistent().get(&BoundaryTokenKey::Balance(id)).unwrap_or(0)
    }
}

// ── Harness ──────────────────────────────────────────────────────────────────

/// Reserve magnitudes 1e3..1e15, offset by small primes so the ratio-derived
/// side almost never divides evenly and the floor actually bites.
const RESERVES: [i128; 5] =
    [1_003, 1_000_003, 1_000_000_007, 1_000_000_000_039, 999_999_999_999_989];

/// Initial mint needs `sqrt(a * b) - MINIMUM_LIQUIDITY >= MIN_LIQUIDITY_FOR_USER`.
const MIN_SEED_PRODUCT: i128 = 4_000_000;

/// Large enough to fund every deposit in a single pool's case list.
const USER_FUNDS: i128 = 1_000_000_000_000_000_000;

struct Pool<'a> {
    router: RouterClient<'a>,
    pair: PairClient<'a>,
    lp: LpTokenClient<'a>,
    token_a: Address,
    token_b: Address,
    user: Address,
}

fn setup_pool<'a>(reserve_a: i128, reserve_b: i128) -> Pool<'a> {
    // One pool runs many invocations; skip the per-test JSON snapshot.
    let env = Env::new_with_config(EnvTestConfig { capture_snapshot_at_drop: false });
    env.mock_all_auths_allowing_non_root_auth();
    env.cost_estimate().budget().reset_unlimited();

    let t1 = env.register(BoundaryToken, ());
    let t2 = env.register(BoundaryToken, ());
    // The factory always initializes pairs with canonically sorted tokens.
    let (token_a, token_b) = if t1 < t2 { (t1, t2) } else { (t2, t1) };

    let factory_id = env.register(MockFactory, ());
    let lp_id = env.register(LpToken, ());
    let pair_id = env.register(Pair, ());
    let router_id = env.register(Router, ());

    let lp = LpTokenClient::new(&env, &lp_id);
    lp.initialize(
        &pair_id,
        &7u32,
        &String::from_str(&env, "Coral LP"),
        &String::from_str(&env, "CLP"),
    );
    let pair = PairClient::new(&env, &pair_id);
    pair.initialize(&factory_id, &token_a, &token_b, &lp_id);
    MockFactoryClient::new(&env, &factory_id).set_pair(&token_a, &token_b, &pair_id);

    let router = RouterClient::new(&env, &router_id);
    router.initialize(&factory_id, &Vec::new(&env));

    // Seed the pool directly through the pair.
    let seeder = Address::generate(&env);
    let tok_a = BoundaryTokenClient::new(&env, &token_a);
    let tok_b = BoundaryTokenClient::new(&env, &token_b);
    tok_a.mint(&seeder, &reserve_a);
    tok_b.mint(&seeder, &reserve_b);
    tok_a.transfer(&seeder, &pair_id, &reserve_a);
    tok_b.transfer(&seeder, &pair_id, &reserve_b);
    pair.mint(&seeder);

    let user = Address::generate(&env);
    tok_a.mint(&user, &USER_FUNDS);
    tok_b.mint(&user, &USER_FUNDS);

    Pool { router, pair, lp, token_a, token_b, user }
}

/// Client-side simulation: the router's own quote, plus the pair's
/// independent acceptance predicate evaluated with the pair's own math.
/// Returns `(amount_a, amount_b, liquidity)` or the error the router surfaces.
fn simulate(
    pool: &Pool,
    desired: (i128, i128),
    mins: (i128, i128),
) -> Result<(i128, i128, i128), Option<RouterError>> {
    let (reserve_a, reserve_b, _) = pool.pair.get_reserves();
    let (amount_a, amount_b) =
        compute_optimal_amounts(desired.0, desired.1, mins.0, mins.1, reserve_a, reserve_b)
            .map_err(Some)?;

    // Pair-side checks; a rejection here surfaces as a pair error, not a
    // router error, so it is reported as `None`.
    if amount_a < MINIMUM_RESERVE || amount_b < MINIMUM_RESERVE {
        return Err(None);
    }
    let supply = pool.lp.total_supply();
    let liquidity =
        subsequent_liquidity(amount_a, amount_b, reserve_a, reserve_b, supply).map_err(|_| None)?;
    if liquidity <= 0 || liquidity < MIN_LIQUIDITY_FOR_USER {
        return Err(None);
    }
    Ok((amount_a, amount_b, liquidity))
}

/// Executes `add_liquidity` and reports the outcome in the same shape as
/// [`simulate`]. A failed invocation is rolled back by the test host.
fn execute(
    pool: &Pool,
    desired: (i128, i128),
    mins: (i128, i128),
) -> Result<(i128, i128, i128), Option<RouterError>> {
    match pool.router.try_add_liquidity(
        &pool.token_a,
        &pool.token_b,
        &desired.0,
        &desired.1,
        &mins.0,
        &mins.1,
        &pool.user,
        &u64::MAX,
        &None,
    ) {
        Ok(Ok(out)) => Ok(out),
        Ok(Err(_)) => panic!("add_liquidity returned an unconvertible value"),
        Err(Ok(err)) => Err(router_error_from(err)),
        Err(Err(_)) => Err(None),
    }
}

/// Maps a host error to the router's own error, or `None` for errors raised
/// by the pair / LP token (codes 1xx / 2xx) or the host itself.
fn router_error_from(err: soroban_sdk::Error) -> Option<RouterError> {
    if !err.is_type(soroban_sdk::xdr::ScErrorType::Contract) {
        return None;
    }
    match err.get_code() {
        306 => Some(RouterError::ZeroAmount),
        307 => Some(RouterError::InsufficientLiquidity),
        308 => Some(RouterError::SlippageExceeded),
        314 => Some(RouterError::DustAmount),
        _ => None,
    }
}

/// Desired-amount candidates for a pool: dust-floor edges, fractions of the
/// reserve, and exact / ±1 stroop around the ratio-matched counterpart.
fn cases(reserve_a: i128, reserve_b: i128) -> std::vec::Vec<(i128, i128)> {
    let mut out = std::vec::Vec::new();
    let a_candidates = [
        MINIMUM_RESERVE - 1,
        MINIMUM_RESERVE,
        MINIMUM_RESERVE + 1,
        reserve_a / 7,
        reserve_a / 3 + 1,
        reserve_a,
        reserve_a * 2 + 1,
    ];
    for &a in a_candidates.iter().filter(|a| **a > 0) {
        let b_matched = a * reserve_b / reserve_a;
        for b in [b_matched - 1, b_matched, b_matched + 1, reserve_b * 4 + 3] {
            if b > 0 {
                out.push((a, b));
            }
        }
    }
    out
}

// ── Property ─────────────────────────────────────────────────────────────────

#[test]
fn test_add_liquidity_sim_matches_execution_at_rounding_boundary() {
    let mut executed = 0u32;
    let mut rejected = 0u32;

    for &reserve_a in RESERVES.iter() {
        for &reserve_b in RESERVES.iter() {
            if reserve_a * reserve_b < MIN_SEED_PRODUCT {
                // No pool can be seeded at this magnitude; nothing to quote.
                continue;
            }
            let pool = setup_pool(reserve_a, reserve_b);

            for desired in cases(reserve_a, reserve_b) {
                // 1. Quote with no floors to learn the router's amounts.
                let quote = match simulate(&pool, desired, (0, 0)) {
                    Ok((a, b, _)) => (a, b),
                    Err(_) => {
                        // Rejected quotes must also reject on-chain.
                        let sim = simulate(&pool, desired, (0, 0));
                        let exec = execute(&pool, desired, (0, 0));
                        assert!(
                            exec.is_err(),
                            "sim rejected but execution passed: reserves=({reserve_a},{reserve_b}) desired={desired:?} sim={sim:?} exec={exec:?}"
                        );
                        rejected += 1;
                        continue;
                    }
                };

                // 2. One stroop above the ratio-derived side must revert in
                //    both simulation and execution, without mutating state.
                let over = if quote.0 == desired.0 { (0, quote.1 + 1) } else { (quote.0 + 1, 0) };
                assert_eq!(
                    simulate(&pool, desired, over),
                    Err(Some(RouterError::SlippageExceeded)),
                    "sim over-floor: reserves=({reserve_a},{reserve_b}) desired={desired:?}"
                );
                assert_eq!(
                    execute(&pool, desired, over),
                    Err(Some(RouterError::SlippageExceeded)),
                    "exec over-floor: reserves=({reserve_a},{reserve_b}) desired={desired:?}"
                );

                // 3. Floors pinned exactly at the quote: simulation and
                //    execution must agree bit-for-bit.
                let (res_a0, res_b0, _) = pool.pair.get_reserves();
                let lp0 = pool.lp.balance(&pool.user);
                let sim = simulate(&pool, desired, quote);
                let exec = execute(&pool, desired, quote);
                assert_eq!(
                    sim, exec,
                    "sim/exec divergence: reserves=({reserve_a},{reserve_b}) desired={desired:?} quote={quote:?}"
                );

                match exec {
                    Ok((a, b, liquidity)) => {
                        assert!(a >= quote.0 && b >= quote.1, "min_out not honoured");
                        assert!(a <= desired.0 && b <= desired.1, "desired exceeded");
                        let (res_a1, res_b1, _) = pool.pair.get_reserves();
                        assert_eq!(res_a1 - res_a0, a, "pair saw a different amount_a");
                        assert_eq!(res_b1 - res_b0, b, "pair saw a different amount_b");
                        assert_eq!(pool.lp.balance(&pool.user) - lp0, liquidity);
                        executed += 1;
                    }
                    Err(_) => {
                        let (res_a1, res_b1, _) = pool.pair.get_reserves();
                        assert_eq!(
                            (res_a1, res_b1),
                            (res_a0, res_b0),
                            "failed call mutated reserves"
                        );
                        rejected += 1;
                    }
                }
            }
        }
    }

    // Guard against a vacuous matrix.
    assert!(executed > 100, "too few successful deposits exercised: {executed}");
    assert!(rejected > 0, "dust / floor rejections were never exercised");
}
