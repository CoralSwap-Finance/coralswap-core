use soroban_sdk::{symbol_short, Address, Env};

pub struct ConcentratedPairEvents;

// `deprecated`: Events::publish is superseded by the #[contractevent] macro;
// migration pending alongside the other contracts' event emitters.
#[allow(deprecated)]
impl ConcentratedPairEvents {
    /// Emits the pool-initialization event.
    ///
    /// Topics: `("c_init", token_a, token_b)` so indexers can discover
    /// concentrated pools by token without a factory lookup (`c_` prefix
    /// keeps it distinct from any V2 pair event).
    ///
    /// Data: `(sqrt_price, tick, fee_bps, tick_spacing)`.
    pub fn initialize(
        env: &Env,
        token_a: &Address,
        token_b: &Address,
        sqrt_price: u128,
        tick: i32,
        fee_bps: u32,
        tick_spacing: u32,
    ) {
        env.events().publish(
            (symbol_short!("c_init"), token_a, token_b),
            (sqrt_price, tick, fee_bps, tick_spacing),
        );
    }
}
