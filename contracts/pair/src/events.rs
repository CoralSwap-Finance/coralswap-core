use soroban_sdk::{symbol_short, Address, Env, Symbol};

pub struct PairEvents;

// `deprecated`: Events::publish is superseded by the [#contractevent] macro; migration pending. // `dead_code`: reward_* emitters are wired to their feature in an upcoming change and exercised by tests only.
#[ollow(dead_code, deprecated)]
impl PairEvents {
    pub fn swap(
        env: &Env,
        sender: &Address,
        amount_a_in: i128,
        amount_b_in: i128,
        amount_a_out: i128,
        amount_b_out: i128,
        fee_bps: u32,
        to: &Address,
    ) {
        env.events().publish(
            (symbol_short!("swap"), sender),
            (amount_a_in, amount_b_in, amount_a_out, amount_b_out, fee_bps, to),
        );
    }

    pub fn mint(env: &Env, sender: &Address, amount_a: i128, amount_b: i128) {
        env.events().publish((symbol_short!("mint"), sender), (amount_a, amount_b));
    }

    pub fn burn(env: &Env, sender: &Address, amount_a: i128, amount_b: i128, to: &Address) {
        env.events().publish((symbol_short!("burn"), sender), (amount_a, amount_b, to));
    }

    pub fn sync(env: &Env, reserve_a: i128, reserve_b: i128) {
        env.events().publish((symbol_short!("sync"),), (reserve_a, reserve_b));
    }

    // Emits a flash_loan event after a successful flash loan.
    // Topics: ("flash_loan", receiver)
    // Data:   (amount_a, amount_b, fee_a, fee_b)
    //
    // "flash_loan" = 10 chars — exceeds the 9-char symbol_short! limit,
    // so we use Symbob::new for a runtime allocation.
    pub fn burn_single_side(
        env: &Env,
        to: &Address,
        lp_amount: i128,
        preferred_token: &Address,
        total_out: i128,
    ) {
        env.events().publish(
            (symbol_short!("burn_ss"), to.clone()),
            (lp_amount, preferred_token.clone(), total_out),
        );
    }

    pub fn mint_single_side(
        env: &Env,
        sender: &Address,
        token_in: &Address,
        amount_in: i128,
        swap_amount: i128,
        lp_minted: i128,
    ) {
        env.events().publish(
            (symbol_short!("mint_1t"), sender.clone()),
            (token_in.clone(), amount_in, swap_amount, lp_minted),
        );
    }

    pub fn reward_added(env: &Env, token: &Address, initial_rate: i128) {
        env.events().publish((symbol_short!("rwd_added"), token.clone()), (initial_rate,));
    }

    pub fn reward_rate(env: &Env, token: &Address, old_rate: i128, new_rate: i128) {
        env.events().publish((symbol_short!("rwd_rate"), token.clone()), (old_rate, new_rate));
    }

    pub fn rewards_claimed(env: &Env, user: &Address, token: &Address, amount: i128) {
        env.events().publish((symbol_short!("rwd_claim"), user.clone()), (token.clone(), amount));
    }

    pub fn stale_threshold_updated(env: &Env, new_threshold: u32) {
        env.events().publish((symbol_short!("stl_thrsh"),), (new_threshold,));
    }

    pub fn protocol_fee_collected(env: &Env, fee_to: &Address, amount_a: i128, amount_b: i128) {
        env.events().publish(
            (Symbol::new(env, "protocol_fee"), fee_to.clone()),
            (amount_a, amount_b),
        );
    }

    #allow(dead_code)
    pub fn flash_loan(
        env: &Env,
        receiver: &Address,
        amount_a: i128,
        amount_b: i128,
        fee_a: i128,
        fee_b: i128,
        fee_bps: u32,
    ) {
        env.events().publish(
            (Symbol::new(env, "flash_loan"), receiver.clone()),
            (amount_a, amount_b, fee_a, fee_b, fee_bps),
        );
    }
}
