use soroban_sdk::{Address, Env};

#[allow(dead_code)]
pub struct FactoryEvents;

// `deprecated`: Events::publish is superseded by the #[contractevent] macro;
// migration pending.
#[allow(dead_code, deprecated)]
impl FactoryEvents {
    pub fn pair_created(
        env: &Env,
        token_a: &Address,
        token_b: &Address,
        pair: &Address,
        pair_index: u32,
    ) {
        let topics = (soroban_sdk::symbol_short!("created"), token_a.clone(), token_b.clone());
        env.events().publish(topics, (pair.clone(), pair_index));
    }

    pub fn paused(env: &Env) {
        env.events().publish((soroban_sdk::symbol_short!("paused"),), ());
    }

    pub fn unpaused(env: &Env) {
        env.events().publish((soroban_sdk::symbol_short!("unpaused"),), ());
    }

    pub fn upgrade_proposed(env: &Env, new_wasm_hash: &[u8; 32]) {
        env.events().publish(
            (soroban_sdk::symbol_short!("prop_upg"),),
            soroban_sdk::BytesN::from_array(env, new_wasm_hash),
        );
    }

    pub fn upgrade_executed(env: &Env, new_version: u32) {
        env.events().publish((soroban_sdk::symbol_short!("upgraded"),), new_version);
    }

    pub fn fee_to_set(env: &Env, new_fee_to: &Option<Address>) {
        env.events().publish((soroban_sdk::symbol_short!("fee_to"),), new_fee_to.clone());
    }

    pub fn fee_to_setter_set(env: &Env, new_setter: &Address) {
        env.events().publish((soroban_sdk::symbol_short!("setter"),), new_setter.clone());
    }

    pub fn protocol_fee_updated(
        env: &Env,
        old_fee_bps: u32,
        new_fee_bps: u32,
        fee_to: &Option<Address>,
    ) {
        env.events().publish(
            (soroban_sdk::symbol_short!("fee_upd"),),
            (old_fee_bps, new_fee_bps, fee_to.clone()),
        );
    }

    /// Emitted by `Factory::set_pair_fee` whenever a per-pair fee override is
    /// installed or updated (issue #132). `ledger` is the current ledger
    /// sequence at the time the override took effect.
    pub fn pair_fee_override_set(
        env: &Env,
        pair: &Address,
        old_fee_bps: u32,
        new_fee_bps: u32,
        ledger: u32,
    ) {
        env.events().publish(
            (soroban_sdk::symbol_short!("pair_fee"), pair.clone()),
            (old_fee_bps, new_fee_bps, ledger),
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Compile-time naming-convention checks (issue #382).
//
// Every emitted topic symbol must be lowercase snake_case. Names of ≤ 9
// chars are emitted via `symbol_short!` (the macro itself enforces the
// length limit); longer names use the full snake_case spelling via
// `Symbol::new`. The list below mirrors every literal used by the emitters
// above (plus the `protocol_fee_collected` topic emitted inline in lib.rs)
// — when a symbol is added or renamed, update it here so the build fails
// if the convention is broken. See docs/NAMING_CONVENTIONS.md.
// ─────────────────────────────────────────────────────────────────────────

const fn is_convention_symbol(name: &str) -> bool {
    let bytes = name.as_bytes();
    if bytes.is_empty() {
        return false;
    }
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if !(b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_') {
            return false;
        }
        i += 1;
    }
    true
}

const _: () = assert!(is_convention_symbol("created"));
const _: () = assert!(is_convention_symbol("paused"));
const _: () = assert!(is_convention_symbol("unpaused"));
const _: () = assert!(is_convention_symbol("prop_upg"));
const _: () = assert!(is_convention_symbol("upgraded"));
const _: () = assert!(is_convention_symbol("fee_to"));
const _: () = assert!(is_convention_symbol("setter"));
const _: () = assert!(is_convention_symbol("fee_upd"));
const _: () = assert!(is_convention_symbol("pair_fee"));
const _: () = assert!(is_convention_symbol("protocol_fee_collected"));
