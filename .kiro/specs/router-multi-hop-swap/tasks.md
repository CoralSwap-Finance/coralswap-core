# Implementation Plan

- [x] 1. Implement helper functions for swap calculations
  - Implement `get_amount_out` function using constant-product formula with fee support
  - Implement `sort_tokens` function for canonical token ordering
  - Add overflow protection using checked arithmetic
  - _Requirements: 6.1, 6.4, 7.2_

- [ ]* 1.1 Write property test for constant-product calculation
  - **Property 11: Constant-product calculation correctness**
  - **Validates: Requirements 6.1, 6.2, 6.3, 6.4**

- [ ]* 1.2 Write property test for token sorting
  - **Property 12: Canonical token ordering**
  - **Validates: Requirements 7.1, 7.2**

- [x] 2. Implement input validation logic
  - Add deadline validation against current ledger timestamp
  - Add path length validation (must be >= 2)
  - Add amount_in validation (must be > 0)
  - Add path duplicate detection (adjacent tokens must differ)
  - Return appropriate error types for each validation failure
  - _Requirements: 3.1, 3.2, 5.1, 5.2, 5.5_

- [ ]* 2.1 Write property test for deadline validation
  - **Property 5: Deadline validation**
  - **Validates: Requirements 3.1, 3.2, 3.3**

- [ ]* 2.2 Write property test for invalid path rejection
  - **Property 9: Invalid path rejection**
  - **Validates: Requirements 5.1, 5.5**

- [x] 3. Implement Factory integration for pair address retrieval
  - Create Factory client interface
  - Implement pair address lookup for each hop in the path
  - Handle None case with PairNotFound error
  - Cache pair addresses for swap execution phase
  - _Requirements: 8.1, 8.2, 8.3, 8.4_

- [ ]* 3.1 Write property test for Factory integration
  - **Property 13: Factory integration consistency**
  - **Validates: Requirements 8.1, 8.3, 8.4**

- [ ]* 3.2 Write property test for non-existent pair handling
  - **Property 10: Non-existent pair handling**
  - **Validates: Requirements 5.4, 8.2**

- [x] 4. Implement output amount calculation for all hops
  - Initialize amounts vector with amount_in
  - Loop through path pairs to calculate each hop's output
  - Query reserves and fee from each Pair contract
  - Call get_amount_out for each hop
  - Store calculated amounts in vector
  - _Requirements: 1.2, 1.3, 6.1, 6.2, 6.3_

- [ ]* 4.1 Write property test for sequential hop execution
  - **Property 1: Sequential hop execution**
  - **Validates: Requirements 1.1, 1.2**

- [ ]* 4.2 Write property test for amount chaining
  - **Property 2: Amount chaining consistency**
  - **Validates: Requirements 1.3, 1.4**

- [ ]* 4.3 Write property test for positive intermediate outputs
  - **Property 4: Positive intermediate outputs**
  - **Validates: Requirements 2.4**

- [x] 5. Implement slippage protection check
  - Compare final calculated amount against amount_out_min
  - Return InsufficientOutputAmount error if check fails
  - Proceed to swap execution if check passes
  - _Requirements: 2.1, 2.2, 2.3_

- [ ]* 5.1 Write property test for slippage protection
  - **Property 3: Slippage protection enforcement**
  - **Validates: Requirements 2.1, 2.2, 2.3**

- [x] 6. Implement initial token transfer
  - Get caller address using env.invoker()
  - Transfer amount_in of path[0] tokens from caller to first Pair
  - Use transfer_from with proper authorization
  - Handle transfer errors appropriately
  - _Requirements: 4.1_

- [ ]* 6.1 Write property test for initial token transfer
  - **Property 6: Initial token transfer**
  - **Validates: Requirements 4.1**

- [x] 7. Implement swap execution loop
  - Iterate through each hop in the path
  - Determine destination address (next Pair or final recipient)
  - Sort tokens to determine swap direction
  - Call Pair::swap with correct amount_a_out and amount_b_out parameters
  - Route intermediate outputs to next Pair, final output to 'to' address
  - _Requirements: 1.1, 4.2, 4.3, 7.1_

- [ ]* 7.1 Write property test for routing destinations
  - **Property 7: Correct routing destinations**
  - **Validates: Requirements 4.2, 4.3**

- [ ]* 7.2 Write property test for final recipient balance
  - **Property 8: Final recipient balance increase**
  - **Validates: Requirements 4.4**

- [ ] 8. Integrate all components into swap_exact_tokens_for_tokens
  - Wire together validation, calculation, and execution phases
  - Ensure proper error propagation throughout
  - Return amounts vector on success
  - Add comprehensive inline documentation
  - _Requirements: All_

- [ ]* 8.1 Write unit tests for edge cases
  - Test minimum path length (2 tokens)
  - Test maximum practical path length (5+ tokens)
  - Test boundary amounts (1, max i128)
  - Test zero slippage tolerance
  - Test deadline at exact current timestamp
  - _Requirements: All_

- [ ]* 8.2 Write integration tests with real contracts
  - Deploy Factory, Pair, and Router contracts in test environment
  - Create liquidity pools with known reserves
  - Execute 2-hop and 3-hop swaps end-to-end
  - Verify final balances and event emissions
  - Test with varying dynamic fees
  - _Requirements: All_

- [ ] 9. Checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.
