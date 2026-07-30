//! Cross-contract client used by the pair to consult price feeds for RWA
//! (yield-bearing) token NAV normalization (issue #128).
//!
//! Price feeds expose a `get_price` method that returns the current NAV per
//! token as a fixed-point value scaled to [`PRICE_SCALE`] (1e18).
//! For standard tokens without a price feed the effective price is implicitly
//! `1.0` (i.e., `PRICE_SCALE`).

#![allow(dead_code)]

use soroban_sdk::{contractclient, Address, Env};

/// Fixed-point scale for price feed values (1e18).
/// All price feeds must return NAV ratios in this scale.
pub const PRICE_SCALE: i128 = 1_000_000_000_000_000_000; // 1e18

/// The expected interface that a price feed oracle must implement.
///
/// RWA tokens (e.g. Centrifuge RWAs: deJTRSY, deJAAA) have a NAV that grows
/// over time as yield accrues. The price feed returns the current NAV per
/// token, enabling the pair to normalise reserves before applying the
/// constant-product formula.
#[contractclient(name = "PriceFeedClient")]
pub trait PriceFeedInterface {
    /// Returns the current price / NAV per token scaled to [`PRICE_SCALE`].
    ///
    /// At initial pair creation the feed should return `PRICE_SCALE` (1.0).
    /// As NAV accrues, the returned value increases proportionally.
    ///
    /// # Errors
    /// The pair expects this function to return `Ok(i128)`; a failed call is
    /// treated as an invalid price feed and the operation will revert.
    fn get_price(env: Env) -> i128;
}

