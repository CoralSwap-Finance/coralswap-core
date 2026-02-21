# Requirements Document

## Introduction

This specification defines the implementation of multi-hop token swapping functionality for the Router contract in a Soroban-based decentralized exchange (DEX). The Router contract acts as an intermediary that enables users to swap tokens across multiple liquidity pairs in a single transaction, providing optimal routing and slippage protection. The primary function `swap_exact_tokens_for_tokens` allows users to specify an exact input amount and receive a minimum guaranteed output amount through a series of pair contracts.

## Glossary

- **Router**: The smart contract that orchestrates multi-hop token swaps by interacting with multiple Pair contracts
- **Pair**: A liquidity pool contract that holds reserves of two tokens and executes atomic swaps using the constant-product formula
- **Factory**: The contract responsible for creating and tracking Pair contracts
- **Path**: An ordered vector of token addresses representing the swap route (e.g., [TokenA, TokenB, TokenC] for A→B→C)
- **Hop**: A single swap operation between two adjacent tokens in the path
- **Slippage Protection**: A mechanism ensuring the final output meets or exceeds a user-specified minimum amount
- **Deadline**: A timestamp after which the transaction will revert, protecting users from delayed execution
- **Amount In**: The exact quantity of input tokens the user provides for the swap
- **Amount Out Min**: The minimum acceptable quantity of output tokens the user will receive
- **Constant-Product Formula**: The x*y=k invariant used by Pair contracts to calculate swap amounts

## Requirements

### Requirement 1

**User Story:** As a DEX user, I want to swap an exact amount of input tokens for output tokens through multiple hops, so that I can exchange tokens that don't have direct liquidity pairs.

#### Acceptance Criteria

1. WHEN a user calls swap_exact_tokens_for_tokens with a valid path THEN the Router SHALL execute swaps sequentially through each pair in the path
2. WHEN the path contains N token addresses THEN the Router SHALL perform N-1 swap operations
3. WHEN processing each hop THEN the Router SHALL use the output of the previous hop as the input for the next hop
4. WHEN the final swap completes THEN the Router SHALL return a vector containing the amount for each step in the path
5. WHERE the path length is greater than 2 THEN the Router SHALL support multi-hop swaps (e.g., A→B→C)

### Requirement 2

**User Story:** As a DEX user, I want slippage protection on my swaps, so that I don't receive less than my expected minimum output amount.

#### Acceptance Criteria

1. WHEN the final output amount is calculated THEN the Router SHALL compare it against amount_out_min
2. IF the final output amount is less than amount_out_min THEN the Router SHALL revert with InsufficientOutputAmount error
3. WHEN the final output meets or exceeds amount_out_min THEN the Router SHALL complete the transaction successfully
4. WHEN intermediate swap amounts are calculated THEN the Router SHALL ensure each hop produces sufficient output for the next hop

### Requirement 3

**User Story:** As a DEX user, I want my swap to execute before a deadline, so that I'm protected from stale transactions executing at unfavorable prices.

#### Acceptance Criteria

1. WHEN swap_exact_tokens_for_tokens is called THEN the Router SHALL check the current ledger timestamp against the deadline parameter
2. IF the current timestamp exceeds the deadline THEN the Router SHALL revert with Expired error
3. WHEN the deadline has not passed THEN the Router SHALL proceed with the swap execution

### Requirement 4

**User Story:** As a DEX user, I want the Router to handle token transfers correctly, so that my input tokens are transferred and output tokens are received at the correct address.

#### Acceptance Criteria

1. WHEN the swap begins THEN the Router SHALL transfer amount_in of path[0] tokens from the caller to the first Pair contract
2. WHEN processing intermediate hops THEN the Router SHALL direct each Pair to send output tokens to the next Pair contract
3. WHEN processing the final hop THEN the Router SHALL direct the last Pair to send output tokens to the 'to' address
4. WHEN all transfers complete THEN the Router SHALL ensure the 'to' address receives the final output tokens

### Requirement 5

**User Story:** As a DEX user, I want proper validation of my swap parameters, so that I receive clear error messages for invalid inputs.

#### Acceptance Criteria

1. WHEN the path length is less than 2 THEN the Router SHALL revert with InvalidPath error
2. WHEN amount_in is zero or negative THEN the Router SHALL revert with ZeroAmount error
3. WHEN amount_out_min is negative THEN the Router SHALL revert with InsufficientOutputAmount error
4. WHEN any pair in the path does not exist THEN the Router SHALL revert with PairNotFound error
5. WHEN path[i] equals path[i+1] for any i THEN the Router SHALL revert with IdenticalTokens error

### Requirement 6

**User Story:** As a developer integrating with the Router, I want to calculate expected output amounts before executing swaps, so that I can provide accurate quotes to users.

#### Acceptance Criteria

1. WHEN calculating output for a hop THEN the Router SHALL use the get_amount_out helper function with current reserves and fee
2. WHEN reserves are retrieved THEN the Router SHALL call get_reserves on the Pair contract
3. WHEN the fee is needed THEN the Router SHALL call get_current_fee_bps on the Pair contract
4. WHEN the constant-product formula is applied THEN the Router SHALL account for the dynamic fee in the calculation

### Requirement 7

**User Story:** As a DEX user, I want the Router to interact correctly with Pair contracts, so that swaps execute atomically and emit proper events.

#### Acceptance Criteria

1. WHEN calling a Pair's swap function THEN the Router SHALL provide correct amount_a_out and amount_b_out parameters based on token ordering
2. WHEN tokens are ordered THEN the Router SHALL use the canonical ordering (token_a < token_b) established by the Factory
3. WHEN a swap executes THEN the Pair contract SHALL emit a Swap event with the correct parameters
4. WHEN the Router completes all hops THEN each Pair SHALL have emitted its respective Swap event

### Requirement 8

**User Story:** As a system architect, I want the Router to retrieve Pair addresses from the Factory, so that the system maintains a single source of truth for pair locations.

#### Acceptance Criteria

1. WHEN the Router needs a Pair address THEN the Router SHALL call Factory::get_pair with the two token addresses
2. IF the Factory returns None for a token pair THEN the Router SHALL revert with PairNotFound error
3. WHEN the Factory returns a Pair address THEN the Router SHALL use that address for subsequent swap calls
4. WHEN processing multiple hops THEN the Router SHALL retrieve each required Pair address from the Factory
