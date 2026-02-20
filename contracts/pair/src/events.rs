use soroban_sdk::{symbol_short, Address, Env, Symbol};

pub struct PairEvents;

impl PairEvents {
    pub fn swap(
        _env: &Env, _sender: &Address,
        _amount_a_in: i128, _amount_b_in: i128,
        _amount_a_out: i128, _amount_b_out: i128,
        _fee_bps: u32, _to: &Address,
    ) { todo!() }

    pub fn mint(_env: &Env, _sender: &Address, _amount_a: i128, _amount_b: i128) { todo!() }
    pub fn burn(_env: &Env, _sender: &Address, _amount_a: i128, _amount_b: i128, _to: &Address) { todo!() }
    pub fn sync(_env: &Env, _reserve_a: i128, _reserve_b: i128) { todo!() }

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
