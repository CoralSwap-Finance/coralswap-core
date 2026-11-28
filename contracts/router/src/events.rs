use soroban_sdk::{symbol_short, Address, Env};

pub struct RouterEvents;

impl RouterEvents {
    /// Topics: `("sweep", token)`. Data: `(recipient, amount)`.
    pub fn sweep(env: &Env, token: &Address, recipient: &Address, amount: i128) {
        env.events().publish(
            (symbol_short!("sweep"), token.clone()),
            (recipient.clone(), amount),
        );
    }
}