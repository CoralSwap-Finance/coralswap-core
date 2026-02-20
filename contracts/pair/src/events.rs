use soroban_sdk::{symbol_short, Address, Env, Symbol};

pub struct PairEvents;

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
        env.events().publish(
            (symbol_short!("mint"), sender),
            (amount_a, amount_b),
        );
    }

    pub fn burn(env: &Env, sender: &Address, amount_a: i128, amount_b: i128, to: &Address) {
        env.events().publish(
            (symbol_short!("burn"), sender),
            (amount_a, amount_b, to),
        );
    }

    pub fn sync(env: &Env, reserve_a: i128, reserve_b: i128) {
        env.events().publish(
            (symbol_short!("sync"),),
            (reserve_a, reserve_b),
        );
    }

    /// Emits a `flash_loan` event after a successful flash loan.
    ///
    /// Topics: `("pair", "flash_loan")`
    /// Data:   `(receiver, amount_a, amount_b, fee_a, fee_b)`
    pub fn flash_loan(
        env: &Env,
        receiver: &Address,
        amount_a: i128,
        amount_b: i128,
        fee_a: i128,
        fee_b: i128,
    ) {
        env.events().publish(
            // "pair" ≤ 9 chars → compile-time constant; "flash_loan" = 10 chars → runtime symbol
            (symbol_short!("pair"), Symbol::new(env, "flash_loan")),
            (receiver.clone(), amount_a, amount_b, fee_a, fee_b),
        );
    }
}
