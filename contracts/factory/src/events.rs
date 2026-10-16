use soroban_sdk::{contractevent, Address, Env};

/// Emitted by `Factory::freeze_pair` the moment a pool is halted.
///
/// Declared with `#[contractevent]` (the migration target of `publish`, see
/// below) because the payload is a named record rather than an anonymous
/// tuple: indexers need `pair` for filtering, and `by` / `ledger` to reconstruct
/// an incident timeline without a second call.
///
/// - Topics: `("pair_frozen_event", pair)`
/// - Data: map `{ by: Address, ledger: u32 }`
#[contractevent]
#[derive(Clone)]
pub struct PairFrozenEvent {
    /// The pair contract that was frozen.
    #[topic]
    pub pair: Address,
    /// The factory admin (`fee_to_setter`) that authorized the freeze.
    pub by: Address,
    /// Ledger sequence at which the freeze took effect.
    pub ledger: u32,
}

/// Emitted by `Factory::unfreeze_pair`; the mirror of [`PairFrozenEvent`].
///
/// - Topics: `("pair_unfrozen_event", pair)`
/// - Data: map `{ by: Address, ledger: u32 }`
#[contractevent]
#[derive(Clone)]
pub struct PairUnfrozenEvent {
    /// The pair contract that was unfrozen.
    #[topic]
    pub pair: Address,
    /// The factory admin (`fee_to_setter`) that authorized the unfreeze.
    pub by: Address,
    /// Ledger sequence at which the unfreeze took effect.
    pub ledger: u32,
}

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

    pub fn resumed(env: &Env) {
        env.events().publish((soroban_sdk::symbol_short!("resumed"),), ());
    }

    /// Emits a heartbeat sync event for indexers with current pause state and pair count.
    pub fn sync(env: &Env, paused: bool, pair_count: u32) {
        env.events().publish((soroban_sdk::symbol_short!("sync"),), (paused, pair_count));
    }

    pub fn pair_frozen(env: &Env, pair: &Address, by: &Address, ledger: u32) {
        PairFrozenEvent { pair: pair.clone(), by: by.clone(), ledger }.publish(env);
    }

    pub fn pair_unfrozen(env: &Env, pair: &Address, by: &Address, ledger: u32) {
        PairUnfrozenEvent { pair: pair.clone(), by: by.clone(), ledger }.publish(env);
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
// `#[contractevent]` derives the topic symbol from the struct name (snake
// cased) rather than from a literal, so it cannot be caught by the emitters'
// literals above — mirror the derived names here instead.
const _: () = assert!(is_convention_symbol("pair_frozen_event"));
const _: () = assert!(is_convention_symbol("pair_unfrozen_event"));
