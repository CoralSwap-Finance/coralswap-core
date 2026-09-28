# Multi-Asset Pair Design Note

## Current Contract

CoralSwap pools are binary: each Pair is initialized with exactly two distinct
token addresses. Factory creation, reserve accounting, swap inputs/outputs, and
the router path model all rely on that two-token shape. Frontends and SDKs must
not present a single pool as accepting three or more tokens.

## Future Design Direction

Multi-asset support requires a separate protocol design before changing the
current Pair ABI. The design should define:

- Pool identity and deterministic deployment for a canonical, ordered asset
  set, including any fee or curve parameters.
- Reserve representation and per-asset balance invariants, including overflow,
  minimum-liquidity, and unsolicited-transfer handling.
- Swap semantics for selecting input/output assets and calculating fees,
  slippage, and price impact across the full asset set.
- LP share valuation and mint/burn rules, plus router quoting and path encoding.
- Migration and indexing compatibility so existing two-token Pair addresses,
  storage, and events remain stable.

Until that design is implemented and deployed, multi-asset pools are unsupported;
the existing two-token Pair contract remains unchanged.