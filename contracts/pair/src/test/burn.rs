#![cfg(test)]

use coralswap_lp_token::{LpToken, LpTokenClient};

use crate::{Pair, PairClient};
use soroban_sdk::{
    contract, contractimpl, contracttype, testutils::Address as _, Address, Env, String,
};

// ── Minimal mock token ────────────────────────────────────────────────────────

#[contracttype]
enum BurnMockTokenKey {
    Balance(Address),
}

#[contract]
pub struct BurnMockToken;

#[contractimpl]
impl BurnMockToken {
    pub fn mint(env: Env, to: Address, amount: i128) {
        let key = BurnMockTokenKey::Balance(to);
        let bal: i128 = env.storage().persistent().get(&key).unwrap_or(0);
        env.storage().persistent().set(&key, &(bal + amount));
    }

    pub fn transfer(env: Env, from: Address, to: Address, amount: i128) {
        from.require_auth();
        let fk = BurnMockTokenKey::Balance(from);
        let tk = BurnMockTokenKey::Balance(to);
        let fb: i128 = env.storage().persistent().get(&fk).unwrap_or(0);
        let tb: i128 = env.storage().persistent().get(&tk).unwrap_or(0);
        env.storage().persistent().set(&fk, &(fb - amount));
        env.storage().persistent().set(&tk, &(tb + amount));
    }

    pub fn balance(env: Env, id: Address) -> i128 {
        env.storage().persistent().get(&BurnMockTokenKey::Balance(id)).unwrap_or(0)
    }
}

// ── Shared setup ──────────────────────────────────────────────────────────────

#[allow(clippy::type_complexity)]
fn setup_pair(
    reserve_a: i128,
    reserve_b: i128,
) -> (
    Env,
    PairClient<'static>,
    BurnMockTokenClient<'static>,
    BurnMockTokenClient<'static>,
    LpTokenClient<'static>,
    Address,
    Address,
    Address,
) {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let token_a_id = env.register(BurnMockToken, ());
    let token_b_id = env.register(BurnMockToken, ());
    let lp_id = env.register(LpToken, ());
    let pair_id = env.register(Pair, ());

    let token_a = BurnMockTokenClient::new(&env, &token_a_id);
    let token_b = BurnMockTokenClient::new(&env, &token_b_id);
    let lp_client = LpTokenClient::new(&env, &lp_id);
    let pair_client = PairClient::new(&env, &pair_id);

    let admin = Address::generate(&env);
    let factory = Address::generate(&env);
    let user = Address::generate(&env);

    lp_client.initialize(
        &admin,
        &7u32,
        &String::from_str(&env, "Coral LP"),
        &String::from_str(&env, "CLP"),
    );

    pair_client.initialize(&factory, &token_a_id, &token_b_id, &lp_id);

    token_a.mint(&user, &reserve_a);
    token_b.mint(&user, &reserve_b);
    token_a.transfer(&user, &pair_client.address, &reserve_a);
    token_b.transfer(&user, &pair_client.address, &reserve_b);
    pair_client.mint(&user);

    (env, pair_client, token_a, token_b, lp_client, user, token_a_id, token_b_id)
}

fn get_amount_out(amount_in: i128, reserve_in: i128, reserve_out: i128, fee_bps: i128) -> i128 {
    let fee_factor = 10_000 - fee_bps;
    let aif = amount_in * fee_factor;
    aif * reserve_out / (reserve_in * 10_000 + aif)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

// 1. Exit into token_a returns correct amount (balanced pool)
#[test]
fn test_burn_single_side_exit_token_a() {
    let reserve = 1_000_000_000i128;
    let (_env, pair_client, token_a, _token_b, lp_client, user, token_a_id, _token_b_id) =
        setup_pair(reserve, reserve);

    let lp_amount = 10_000_000i128;
    let total_supply = lp_client.total_supply(); // 1_000_000_000

    let share_a = lp_amount * reserve / total_supply; // 10_000_000
    let share_b = lp_amount * reserve / total_supply; // 10_000_000

    let reserve_a_post_burn = reserve - share_a; // 990_000_000
    let reserve_b_post_burn = reserve - share_b; // 990_000_000

    // preferred = token_a, unwanted = token_b (share_b swaps to token_a)
    let swap_out = get_amount_out(share_b, reserve_b_post_burn, reserve_a_post_burn, 30);
    let expected_total = share_a + swap_out; // 10_000_000 + 9_870_596 = 19_870_596

    let result = pair_client.burn_single_side(&user, &lp_amount, &token_a_id, &1i128);

    assert_eq!(result, expected_total, "total_out must equal share_a + swap_out");
    assert_eq!(
        token_a.balance(&user),
        expected_total,
        "user's token_a balance must equal total_out"
    );
}

// 2. Exit into token_b returns correct amount (asymmetric pool)
#[test]
fn test_burn_single_side_exit_token_b() {
    let reserve_a = 1_000_000_000i128;
    let reserve_b = 4_000_000_000i128;
    let (_env, pair_client, _token_a, token_b, lp_client, user, _token_a_id, token_b_id) =
        setup_pair(reserve_a, reserve_b);

    let total_supply = lp_client.total_supply(); // 2_000_000_000
    let lp_amount = 20_000_000i128; // 1% of supply

    let share_a = lp_amount * reserve_a / total_supply; // 10_000_000
    let share_b = lp_amount * reserve_b / total_supply; // 40_000_000

    let reserve_a_post_burn = reserve_a - share_a; // 990_000_000
    let reserve_b_post_burn = reserve_b - share_b; // 3_960_000_000

    // preferred = token_b, unwanted = token_a (share_a swaps to token_b)
    let swap_out = get_amount_out(share_a, reserve_a_post_burn, reserve_b_post_burn, 30);
    let expected_total = share_b + swap_out; // 40_000_000 + 39_482_384 = 79_482_384

    let result = pair_client.burn_single_side(&user, &lp_amount, &token_b_id, &1i128);

    assert_eq!(result, expected_total, "total_out must equal share_b + swap_out_of_a");
    assert_eq!(
        token_b.balance(&user),
        expected_total,
        "user's token_b balance must equal total_out"
    );
}

// 3. K invariant holds post-operation
#[test]
fn test_burn_single_side_k_invariant_holds() {
    let reserve = 1_000_000_000i128;
    let (_env, pair_client, _token_a, _token_b, _lp_client, user, token_a_id, _token_b_id) =
        setup_pair(reserve, reserve);

    let lp_amount = 10_000_000i128;

    pair_client.burn_single_side(&user, &lp_amount, &token_a_id, &1i128);

    let (res_a, res_b, _) = pair_client.get_reserves();

    assert!(res_a > 0 && res_b > 0, "reserves must remain positive");

    // preferred = token_a: reserve_b (unwanted) is net-unchanged after burn+re-add
    assert_eq!(res_b, reserve, "unwanted reserve (token_b) must be unchanged");

    // preferred reserve decreased by total_out
    assert!(res_a < reserve, "preferred reserve (token_a) must decrease");

    // K is well-defined and non-zero
    let _k = res_a.checked_mul(res_b).expect("k must not overflow");
}

// 4. min_amount_out reverts when output is insufficient
#[test]
fn test_burn_single_side_slippage_reverts() {
    let reserve = 1_000_000_000i128;
    let (_env, pair_client, _token_a, _token_b, lp_client, user, token_a_id, _token_b_id) =
        setup_pair(reserve, reserve);

    let lp_amount = 10_000_000i128;
    let total_supply = lp_client.total_supply();

    let share_a = lp_amount * reserve / total_supply;
    let share_b = lp_amount * reserve / total_supply;
    let swap_out = get_amount_out(share_b, reserve - share_b, reserve - share_a, 30);
    let actual_out = share_a + swap_out;

    let result = pair_client.try_burn_single_side(
        &user,
        &lp_amount,
        &token_a_id,
        &(actual_out + 1), // demand 1 stroop more than possible
    );

    assert!(result.is_err(), "must revert when min_amount_out exceeds actual output");
}

// 5. LP token supply decreases by exactly lp_amount
#[test]
fn test_burn_single_side_lp_supply_decreases() {
    let reserve = 1_000_000_000i128;
    let (_env, pair_client, _token_a, _token_b, lp_client, user, token_a_id, _token_b_id) =
        setup_pair(reserve, reserve);

    let supply_before = lp_client.total_supply();
    let lp_amount = 10_000_000i128;

    pair_client.burn_single_side(&user, &lp_amount, &token_a_id, &1i128);

    let supply_after = lp_client.total_supply();

    assert_eq!(
        supply_before - supply_after,
        lp_amount,
        "LP total_supply must decrease by exactly lp_amount"
    );
}

// ── Issue #289: MINIMUM_LIQUIDITY seed protection tests ─────────────────────

use crate::math::MINIMUM_LIQUIDITY;

#[test]
fn test_burn_seed_remains_intact_after_full_cycle() {
    let reserve = 1_000_000_000i128;
    let (_env, pair_client, _token_a, _token_b, lp_client, user, _token_a_id, _token_b_id) =
        setup_pair(reserve, reserve);

    let total_supply = lp_client.total_supply();
    let user_balance = lp_client.balance(&user);

    // User burns all their LP tokens (but not the seed held by the contract)
    lp_client.transfer(&user, &pair_client.address, &user_balance);
    let (_amount_a, _amount_b) = pair_client.burn(&user);

    // After burn, the only LP tokens remaining should be the MINIMUM_LIQUIDITY seed
    let remaining_supply = lp_client.total_supply();
    assert_eq!(
        remaining_supply, MINIMUM_LIQUIDITY,
        "Only MINIMUM_LIQUIDITY seed should remain after full burn"
    );

    // The seed should be held by the pair contract
    let contract_balance = lp_client.balance(&pair_client.address);
    assert_eq!(
        contract_balance, MINIMUM_LIQUIDITY,
        "Seed should be permanently locked in the contract"
    );

    // Verify initial supply was correct (user received total - seed)
    assert_eq!(
        user_balance,
        total_supply - MINIMUM_LIQUIDITY,
        "User should have received all LP tokens except the seed"
    );
}

#[test]
fn test_burn_seed_not_redeemable() {
    let reserve = 1_000_000_000i128;
    let (_env, pair_client, token_a, token_b, lp_client, user, _token_a_id, _token_b_id) =
        setup_pair(reserve, reserve);

    let initial_token_a = token_a.balance(&user);
    let initial_token_b = token_b.balance(&user);

    // Record initial reserves
    let (initial_reserve_a, initial_reserve_b, _) = pair_client.get_reserves();

    let user_lp = lp_client.balance(&user);

    // User burns all their LP tokens
    lp_client.transfer(&user, &pair_client.address, &user_lp);
    let (returned_a, returned_b) = pair_client.burn(&user);

    let final_token_a = token_a.balance(&user);
    let final_token_b = token_b.balance(&user);

    // Calculate what proportion of reserves was returned
    let total_supply_before_burn = user_lp + MINIMUM_LIQUIDITY;
    let burnable_supply = total_supply_before_burn - MINIMUM_LIQUIDITY;

    // Expected return should be based on burnable_supply, NOT total_supply
    let expected_a = (user_lp * initial_reserve_a) / burnable_supply;
    let expected_b = (user_lp * initial_reserve_b) / burnable_supply;

    assert_eq!(returned_a, expected_a, "Returned token_a should exclude seed's share");
    assert_eq!(returned_b, expected_b, "Returned token_b should exclude seed's share");

    // Verify user received the correct amounts
    assert_eq!(final_token_a - initial_token_a, returned_a, "User token_a balance mismatch");
    assert_eq!(final_token_b - initial_token_b, returned_b, "User token_b balance mismatch");

    // The remaining reserves should be the seed's proportional share
    let (final_reserve_a, final_reserve_b, _) = pair_client.get_reserves();
    assert!(final_reserve_a > 0, "Some reserves should remain for the seed");
    assert!(final_reserve_b > 0, "Some reserves should remain for the seed");
}

#[test]
fn test_burn_single_side_seed_remains_intact() {
    let reserve = 1_000_000_000i128;
    let (_env, pair_client, _token_a, _token_b, lp_client, user, token_a_id, _token_b_id) =
        setup_pair(reserve, reserve);

    let user_lp = lp_client.balance(&user);

    // Burn most of user's LP in single-side mode
    let burn_amount = user_lp - 10_000i128; // Leave a tiny bit
    let _returned = pair_client.burn_single_side(&user, &burn_amount, &token_a_id, &1i128);

    // Burn the rest
    let remaining = lp_client.balance(&user);
    if remaining > 0 {
        let _final_return = pair_client.burn_single_side(&user, &remaining, &token_a_id, &1i128);
    }

    // After all burns, seed should still exist
    let final_supply = lp_client.total_supply();
    assert_eq!(
        final_supply, MINIMUM_LIQUIDITY,
        "Seed should remain after all single-side burns"
    );

    let contract_balance = lp_client.balance(&pair_client.address);
    assert_eq!(
        contract_balance, MINIMUM_LIQUIDITY,
        "Seed should be in the contract"
    );
}

#[test]
fn test_burn_cannot_extract_seed_reserves() {
    let reserve = 1_000_000_000i128;
    let (env, pair_client, token_a, token_b, lp_client, user, _token_a_id, _token_b_id) =
        setup_pair(reserve, reserve);

    // Create a second user who will try to claim seed reserves
    let attacker = Address::generate(&env);

    // Attacker has no LP tokens
    assert_eq!(lp_client.balance(&attacker), 0);

    // Attacker cannot burn without LP tokens
    let result = pair_client.try_burn(&attacker);
    assert!(result.is_err(), "Cannot burn without LP tokens");

    // Even if attacker somehow transfers 0 LP to contract, they get nothing
    // (This tests the lp_balance check in burn)
    let attacker_tokens_a_before = token_a.balance(&attacker);
    let attacker_tokens_b_before = token_b.balance(&attacker);

    // The contract holds MINIMUM_LIQUIDITY seed LP tokens
    let contract_lp = lp_client.balance(&pair_client.address);
    assert_eq!(contract_lp, MINIMUM_LIQUIDITY);

    // Attempt to burn with 0 LP in contract (as attacker) should fail
    let result = pair_client.try_burn(&attacker);
    assert!(result.is_err(), "Burn with 0 LP should fail");

    // Verify attacker received nothing
    assert_eq!(token_a.balance(&attacker), attacker_tokens_a_before);
    assert_eq!(token_b.balance(&attacker), attacker_tokens_b_before);

    // Seed remains intact
    assert_eq!(lp_client.balance(&pair_client.address), MINIMUM_LIQUIDITY);
}
