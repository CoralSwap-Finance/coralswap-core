use soroban_sdk::{symbol_short, Address, Env, Symbol};

pub struct PairEvents;

// `deprecated`: Events::publish is superseded by the #[contractevent] macro;
// migration pending. `dead_code`: reward_* emitters are wired to their
// feature in an upcoming change and exercised by tests only.
#[allow(dead_code, deprecated)]
impl PairEvents {
    /// Emits a `swap` event after a successful token swap.
    ///
    /// Topics: `("swap", sender)`
    /// Data:   `(amount_a_in, amount_b_in, amount_a_out, amount_b_out, fee_bps, to)`
    ///
    /// Mirrors Uniswap V2 Swap semantics but with i128 amounts and an
    /// explicit `fee_bps` field to expose the dynamic fee to indexers.
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

    // Emits a `flash_loan` event after a successful flash loan.

    // Topics: `("pair", "flash_loan")`
    // Data:   `(receiver, amount_a, amount_b, fee_a, fee_b)`
    /// Emits a `flash_loan` event after a successful flash loan.
    ///
    /// Topics: `("flash_loan", receiver)`
    /// Data:   `(amount_a, amount_b, fee_a, fee_b)`
    ///
    /// "flash_loan" = 10 chars → exceeds the 9-char symbol_short! limit,
    /// so we use Symbol::new for a runtime allocation.
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

    /// Emitted by `Pair::mint_with_one_token` after a successful single-sided
    /// liquidity deposit.
    ///
    /// Topics: `("mint_1t", sender)`
    /// Data:   `(token_in, amount_in, swap_amount, lp_minted)`
    ///
    /// `swap_amount` is the portion of `amount_in` that was swapped internally
    /// to obtain the complementary token before minting.
    ///
    /// "mint_1t" = 6 chars — fits `symbol_short!` (≤ 9 chars).
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

    /// Emits a `reward_added` event when a reward token is added.
    ///
    /// Topics: `("rwd_added", token)`
    /// Data:   `(initial_rate,)`
    pub fn reward_added(env: &Env, token: &Address, initial_rate: i128) {
        env.events().publish((symbol_short!("rwd_added"), token.clone()), (initial_rate,));
    }

    /// Emits a `reward_rate` event when a reward rate changes.
    ///
    /// Topics: `("rwd_rate", token)`
    /// Data:   `(old_rate, new_rate)`
    pub fn reward_rate(env: &Env, token: &Address, old_rate: i128, new_rate: i128) {
        env.events().publish((symbol_short!("rwd_rate"), token.clone()), (old_rate, new_rate));
    }

    /// Emits a `rewards_claimed` event when a user claims rewards.
    ///
    /// Topics: `("rwd_claim", user)`
    /// Data:   `(token, amount)`
    pub fn rewards_claimed(env: &Env, user: &Address, token: &Address, amount: i128) {
        env.events().publish((symbol_short!("rwd_claim"), user.clone()), (token.clone(), amount));
    }

    /// Emits a `stale_threshold_updated` event when the EMA staleness decay
    /// threshold is reconfigured.
    ///
    /// Topics: `("stl_thrsh",)`
    /// Data:   `(new_threshold,)`
    ///
    /// Called by `Pair::set_stale_threshold()` after a successful threshold update.
    /// "stale_threshold_updated" is too long for symbol_short!, so we use
    /// "stl_thrsh" (9 chars).
    pub fn stale_threshold_updated(env: &Env, new_threshold: u32) {
        env.events().publish((symbol_short!("stl_thrsh"),), (new_threshold,));
    }

    #[allow(dead_code)]
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
