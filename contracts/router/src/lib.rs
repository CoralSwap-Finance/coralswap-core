#![cfg_attr(not(test), no_std)]

#[cfg(test)]
extern crate std;

mod errors;
mod helpers;
mod storage;

#[cfg(test)]
mod test;

use errors::RouterError;
use helpers::{
    compute_optimal_amounts, get_amount_in, get_amount_out, get_pair_address,
    get_pair_reserves_and_fee, get_path_amounts_out, sort_tokens, FactoryClient, PairClient,
};
use soroban_sdk::{
    contract, contractimpl, token::TokenClient, xdr::ToXdr, Address, Bytes, BytesN, Env, Vec,
};
use storage::{
    clear_commit, get_commit, get_commit_config, get_commit_count, get_factory, get_hubs,
    is_nonce_used, set_commit, set_commit_config, set_commit_count, set_factory, set_hubs,
    set_nonce_used, CommitEntry, RouterCommitConfig,
};

// Shared TTL policy (issue #390): INSTANCE_TTL_THRESHOLD = 60_480 (~3.5d), EXTEND_TO = 120_960 (~7d).
// Previously local magic 50_000 / 120_960 — now unified via coralswap-shared.
use coralswap_shared::{
    INSTANCE_TTL_EXTEND_TO as TTL_EXTEND_TO, INSTANCE_TTL_THRESHOLD as TTL_THRESHOLD,
};

/// Computes `sha256(sender || token_in || token_out || amount_in || min_out || nonce || salt)`.
///
/// Fields are concatenated in XDR/big-endian encoding so the result is deterministic
/// and reproducible by off-chain clients building the commitment hash.
fn compute_swap_hash(
    env: &Env,
    sender: &Address,
    token_in: &Address,
    token_out: &Address,
    amount_in: i128,
    min_out: i128,
    nonce: u64,
    salt: &BytesN<32>,
) -> BytesN<32> {
    let mut data = Bytes::new(env);
    data.append(&sender.to_xdr(env));
    data.append(&token_in.to_xdr(env));
    data.append(&token_out.to_xdr(env));
    data.append(&Bytes::from_slice(env, &amount_in.to_be_bytes()));
    data.append(&Bytes::from_slice(env, &min_out.to_be_bytes()));
    data.append(&Bytes::from_slice(env, &nonce.to_be_bytes()));
    let salt_bytes: Bytes = salt.clone().into();
    data.append(&salt_bytes);
    env.crypto().sha256(&data).into()
}

/// Rejects the call if either deadline has passed (issue #365).
///
/// * `deadline` — Unix timestamp (seconds); the call fails once
///   `ledger.timestamp() > deadline`. Pass `u64::MAX` for "no timestamp bound".
/// * `deadline_ledger` — optional ledger sequence number; the call fails once
///   `ledger.sequence() > deadline_ledger`. `None` means "no ledger bound", so
///   existing callers keep their behaviour by passing `None`.
///
/// Both bounds are checked up front, before the first hop touches any pair or
/// token. A Soroban invocation executes inside a single ledger, so the ledger
/// and timestamp cannot advance between hops: one check at the first hop is
/// exactly equivalent to checking every intermediate hop.
fn check_deadline(
    env: &Env,
    deadline: u64,
    deadline_ledger: Option<u32>,
) -> Result<(), RouterError> {
    if deadline < env.ledger().timestamp() {
        return Err(RouterError::Expired);
    }
    if let Some(limit) = deadline_ledger {
        if env.ledger().sequence() > limit {
            return Err(RouterError::Expired);
        }
    }
    Ok(())
}

#[contract]
pub struct Router;

#[contractimpl]
impl Router {
    pub fn initialize(env: Env, factory: Address, hubs: Vec<Address>) {
        set_factory(&env, &factory);
        set_hubs(&env, &hubs);
        env.storage().instance().extend_ttl(TTL_THRESHOLD, TTL_EXTEND_TO);
    }

    /// Stores the hub token addresses used for multi-hop path discovery.
    pub fn set_hubs(env: Env, hubs: Vec<Address>) {
        set_hubs(&env, &hubs);
        env.storage().instance().extend_ttl(TTL_THRESHOLD, TTL_EXTEND_TO);
    }

    /// Drains the router's entire balance of `token` to `recipient`, recovering
    /// balances orphaned by a multi-hop path that failed after a transfer.
    ///
    /// Governance-only: requires authorization from the factory's
    /// `fee_to_setter`. Returns the amount swept.
    pub fn sweep(env: Env, token: Address, recipient: Address) -> Result<i128, RouterError> {
        let factory = get_factory(&env).ok_or(RouterError::PairNotFound)?;
        let governance =
            FactoryClient::new(&env, &factory).fee_to_setter().ok_or(RouterError::InternalError)?;
        governance.require_auth();

        let router = env.current_contract_address();
        let token_client = TokenClient::new(&env, &token);
        let amount = token_client.balance(&router);
        if amount <= 0 {
            return Err(RouterError::ZeroAmount);
        }
        token_client.transfer(&router, &recipient, &amount);
        env.storage().instance().extend_ttl(TTL_THRESHOLD, TTL_EXTEND_TO);
        Ok(amount)
    }

    /// Returns the current list of hub token addresses.
    pub fn get_hubs(env: Env) -> Vec<Address> {
        get_hubs(&env)
    }

    /// Finds the best multi-hop route for swapping token_in → token_out.
    /// Evaluates 1-hop (direct), 2-hop (via each hub), and 3-hop (via each
    /// ordered hub pair) paths, selecting the one with highest expected output.
    pub fn get_best_path(
        env: Env,
        token_in: Address,
        token_out: Address,
        amount_in: i128,
    ) -> Result<(Vec<Address>, i128), RouterError> {
        if token_in == token_out {
            return Err(RouterError::IdenticalTokens);
        }
        if amount_in <= 0 {
            return Err(RouterError::ZeroAmount);
        }

        let factory = get_factory(&env).ok_or(RouterError::PairNotFound)?;
        let hubs = get_hubs(&env);

        // Track best across all candidate paths.
        let mut best_path: Vec<Address> = Vec::new(&env);
        let mut best_out: i128 = -1;

        // 1) Direct pair (1 hop)
        if let Ok(pair) = get_pair_address(&env, &factory, &token_in, &token_out) {
            if let Ok((r_in, r_out, fee)) =
                get_pair_reserves_and_fee(&env, &pair, &token_in, &token_out)
            {
                if let Ok(out) = get_amount_out(&env, amount_in, r_in, r_out, fee) {
                    let mut path = Vec::new(&env);
                    path.push_back(token_in.clone());
                    path.push_back(token_out.clone());
                    best_path = path;
                    best_out = out;
                }
            }
        }

        // 2) 2-hop routes: token_in → hub → token_out
        for i in 0..hubs.len() {
            let hub = hubs.get(i).unwrap();
            if hub == token_in || hub == token_out {
                continue;
            }
            let pair_1 = get_pair_address(&env, &factory, &token_in, &hub);
            let pair_2 = get_pair_address(&env, &factory, &hub, &token_out);
            if let (Ok(p1), Ok(p2)) = (pair_1, pair_2) {
                if let (Ok((r1_in, r1_out, f1)), Ok((r2_in, r2_out, f2))) = (
                    get_pair_reserves_and_fee(&env, &p1, &token_in, &hub),
                    get_pair_reserves_and_fee(&env, &p2, &hub, &token_out),
                ) {
                    if let Ok(mid) = get_amount_out(&env, amount_in, r1_in, r1_out, f1) {
                        if let Ok(out) = get_amount_out(&env, mid, r2_in, r2_out, f2) {
                            if out > best_out {
                                let mut path = Vec::new(&env);
                                path.push_back(token_in.clone());
                                path.push_back(hub.clone());
                                path.push_back(token_out.clone());
                                best_path = path;
                                best_out = out;
                            }
                        }
                    }
                }
            }
        }

        // 3) 3-hop routes: token_in → hub_i → hub_j → token_out
        for i in 0..hubs.len() {
            for j in 0..hubs.len() {
                if i == j {
                    continue;
                }
                let hub_i = hubs.get(i).unwrap();
                let hub_j = hubs.get(j).unwrap();
                if hub_i == token_in
                    || hub_i == token_out
                    || hub_j == token_in
                    || hub_j == token_out
                {
                    continue;
                }
                let p1 = get_pair_address(&env, &factory, &token_in, &hub_i);
                let p2 = get_pair_address(&env, &factory, &hub_i, &hub_j);
                let p3 = get_pair_address(&env, &factory, &hub_j, &token_out);
                if let (Ok(p1), Ok(p2), Ok(p3)) = (p1, p2, p3) {
                    if let (
                        Ok((r1_in, r1_out, f1)),
                        Ok((r2_in, r2_out, f2)),
                        Ok((r3_in, r3_out, f3)),
                    ) = (
                        get_pair_reserves_and_fee(&env, &p1, &token_in, &hub_i),
                        get_pair_reserves_and_fee(&env, &p2, &hub_i, &hub_j),
                        get_pair_reserves_and_fee(&env, &p3, &hub_j, &token_out),
                    ) {
                        if let Ok(mid1) = get_amount_out(&env, amount_in, r1_in, r1_out, f1) {
                            if let Ok(mid2) = get_amount_out(&env, mid1, r2_in, r2_out, f2) {
                                if let Ok(out) = get_amount_out(&env, mid2, r3_in, r3_out, f3) {
                                    if out > best_out {
                                        let mut path = Vec::new(&env);
                                        path.push_back(token_in.clone());
                                        path.push_back(hub_i.clone());
                                        path.push_back(hub_j.clone());
                                        path.push_back(token_out.clone());
                                        best_path = path;
                                        best_out = out;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        if best_out < 0 {
            return Err(RouterError::PairNotFound);
        }
        Ok((best_path, best_out))
    }

    /// Swaps an exact amount of input tokens for a minimum amount of output
    /// tokens along a given multi-hop path. Supports 1, 2, or 3 hops.
    ///
    /// The path must have 2 to 4 entries: [token_in, ..., token_out].
    /// Intermediate tokens are sent to and forwarded by this router contract.
    ///
    /// Forward dust (issue #352): after the last hop, any residual balance of
    /// the input token held by the router — e.g. a few stroops stranded by a
    /// previously reverted path, or left by future forwarding changes — is
    /// swept back to `to`, so the router never retains input tokens.
    pub fn swap_exact_tokens_multi_hop(
        env: Env,
        path: Vec<Address>,
        amount_in: i128,
        amount_out_min: i128,
        to: Address,
        deadline: u64,
        deadline_ledger: Option<u32>,
    ) -> Result<i128, RouterError> {
        check_deadline(&env, deadline, deadline_ledger)?;
        if amount_in <= 0 {
            return Err(RouterError::ZeroAmount);
        }
        let hops = path.len() - 1;
        if !(1..=3).contains(&hops) {
            return Err(RouterError::InvalidPath);
        }

        let factory = get_factory(&env).ok_or(RouterError::PairNotFound)?;
        let amounts = get_path_amounts_out(&env, &factory, &path, amount_in)?;
        let final_out = amounts.get(amounts.len() - 1).unwrap();

        if final_out < amount_out_min {
            return Err(RouterError::InsufficientOutputAmount);
        }

        to.require_auth();
        let router = env.current_contract_address();

        // Transfer user input to the first pair
        let token_in = path.get(0).unwrap();
        let first_pair = get_pair_address(&env, &factory, &token_in, &path.get(1).unwrap())?;
        TokenClient::new(&env, &token_in).transfer(&to, &first_pair, &amount_in);

        // Execute each hop in sequence
        for i in 0..hops {
            let token_from = path.get(i).unwrap();
            let token_to = path.get(i + 1).unwrap();
            let pair = get_pair_address(&env, &factory, &token_from, &token_to)?;
            let amount_out_hop = amounts.get(i).unwrap();
            let dest = if i + 1 == hops { &to } else { &router };

            let (token_0, _) = sort_tokens(&token_from, &token_to)?;
            let pair_client = PairClient::new(&env, &pair);
            if token_from == token_0 {
                pair_client.swap(&0, &amount_out_hop, dest);
            } else {
                pair_client.swap(&amount_out_hop, &0, dest);
            }

            // Forward the intermediate output to the next pair
            if i + 1 < hops {
                let next_pair =
                    get_pair_address(&env, &factory, &token_to, &path.get(i + 2).unwrap())?;
                TokenClient::new(&env, &token_to).transfer(&router, &next_pair, &amount_out_hop);
            }
        }

        // Sweep forward dust back to the recipient (issue #352). Exact-in
        // rounding and previously reverted paths can strand a few stroops of
        // the input token on the router; drain whatever remains so no balance
        // is ever stuck on the router after a swap.
        let dust = TokenClient::new(&env, &token_in).balance(&router);
        if dust > 0 {
            TokenClient::new(&env, &token_in).transfer(&router, &to, &dust);
        }

        Ok(final_out)
    }

    /// Swaps an exact amount of input tokens for a minimum amount of output
    /// tokens along a given path. Supports 1, 2, or 3 hops.
    pub fn swap_exact_tokens_for_tokens(
        env: Env,
        amount_in: i128,
        amount_out_min: i128,
        path: Vec<Address>,
        to: Address,
        deadline: u64,
        deadline_ledger: Option<u32>,
    ) -> Result<Vec<i128>, RouterError> {
        let final_out = Self::swap_exact_tokens_multi_hop(
            env.clone(),
            path,
            amount_in,
            amount_out_min,
            to,
            deadline,
            deadline_ledger,
        )?;
        let mut amounts = Vec::new(&env);
        amounts.push_back(final_out);
        Ok(amounts)
    }

    /// Swaps tokens to receive an exact amount of output tokens.
    /// Computes required input along the given path and enforces amount_in_max.
    pub fn swap_tokens_for_exact_tokens(
        env: Env,
        amount_out: i128,
        amount_in_max: i128,
        path: Vec<Address>,
        to: Address,
        deadline: u64,
        deadline_ledger: Option<u32>,
    ) -> Result<Vec<i128>, RouterError> {
        check_deadline(&env, deadline, deadline_ledger)?;
        if amount_out <= 0 {
            return Err(RouterError::ZeroAmount);
        }
        let hops = path.len() - 1;
        if !(1..=3).contains(&hops) {
            return Err(RouterError::InvalidPath);
        }

        let factory = get_factory(&env).ok_or(RouterError::PairNotFound)?;

        // Walk backwards: compute required input for each hop from final output
        let mut required = Vec::new(&env);
        let mut current = amount_out;
        for i in (0..hops).rev() {
            let pair =
                get_pair_address(&env, &factory, &path.get(i).unwrap(), &path.get(i + 1).unwrap())?;
            let (reserve_in, reserve_out, fee_bps) = get_pair_reserves_and_fee(
                &env,
                &pair,
                &path.get(i).unwrap(),
                &path.get(i + 1).unwrap(),
            )?;
            current = get_amount_in(&env, current, reserve_in, reserve_out, fee_bps)?;
            required.insert(0, current);
        }
        let amount_in_needed = current;

        if amount_in_needed > amount_in_max {
            return Err(RouterError::ExcessiveInputAmount);
        }

        to.require_auth();
        let router = env.current_contract_address();

        // Transfer user input to the first pair
        let token_in = path.get(0).unwrap();
        let first_pair = get_pair_address(&env, &factory, &token_in, &path.get(1).unwrap())?;
        TokenClient::new(&env, &token_in).transfer(&to, &first_pair, &amount_in_needed);

        // Execute each hop forward
        for i in 0..hops {
            let token_from = path.get(i).unwrap();
            let token_to = path.get(i + 1).unwrap();
            let pair = get_pair_address(&env, &factory, &token_from, &token_to)?;
            let _amount_in_this = required.get(i).unwrap();
            let dest = if i + 1 == hops { &to } else { &router };

            let (token_0, _) = sort_tokens(&token_from, &token_to)?;
            let pair_client = PairClient::new(&env, &pair);

            let out_expected = if i + 1 < hops { required.get(i + 1).unwrap() } else { amount_out };

            if token_from == token_0 {
                pair_client.swap(&0, &out_expected, dest);
            } else {
                pair_client.swap(&out_expected, &0, dest);
            }

            // Forward intermediate output to the next pair
            if i + 1 < hops {
                let next_pair =
                    get_pair_address(&env, &factory, &token_to, &path.get(i + 2).unwrap())?;
                TokenClient::new(&env, &token_to).transfer(&router, &next_pair, &out_expected);
            }
        }

        let mut result = Vec::new(&env);
        result.push_back(amount_in_needed);
        Ok(result)
    }

    /// Adds liquidity to a token pair.
    ///
    /// Deposits an optimal pair of amounts preserving the pool ratio, then
    /// mints LP tokens to `to`. Returns `(amount_a, amount_b, liquidity)`.
    ///
    /// # Arguments
    /// * `token_a` - First token address
    /// * `token_b` - Second token address
    /// * `amount_a_desired` - Desired amount of token_a to add
    /// * `amount_b_desired` - Desired amount of token_b to add
    /// * `amount_a_min` - Minimum amount of token_a to add
    /// * `amount_b_min` - Minimum amount of token_b to add
    /// * `to` - Recipient of LP tokens
    /// * `deadline` - Unix timestamp after which the transaction will revert
    /// * `deadline_ledger` - Optional ledger sequence after which the transaction will
    ///   revert; `None` disables the ledger bound (see `check_deadline`)
    pub fn add_liquidity(
        env: Env,
        token_a: Address,
        token_b: Address,
        amount_a_desired: i128,
        amount_b_desired: i128,
        amount_a_min: i128,
        amount_b_min: i128,
        to: Address,
        deadline: u64,
        deadline_ledger: Option<u32>,
    ) -> Result<(i128, i128, i128), RouterError> {
        check_deadline(&env, deadline, deadline_ledger)?;

        // Validate inputs: reject zero desired amounts
        if amount_a_desired <= 0 || amount_b_desired <= 0 {
            return Err(RouterError::ZeroAmount);
        }

        // Validate inputs: reject identical tokens
        if token_a == token_b {
            return Err(RouterError::IdenticalTokens);
        }

        // Issue #357: Ensure tokens are in canonical order to match pair's storage.
        // Pairs store tokens as (token_0, token_1) where token_0 < token_1.
        // If caller provides tokens in reverse order, we must swap amounts
        // to match the pair's reserve ordering.
        let (token_0, token_1) = sort_tokens(&token_a, &token_b)?;
        let swapped = token_0 != token_a;
        let (amount_0_desired, amount_1_desired, amount_0_min, amount_1_min) = if swapped {
            (amount_b_desired, amount_a_desired, amount_b_min, amount_a_min)
        } else {
            (amount_a_desired, amount_b_desired, amount_a_min, amount_b_min)
        };

        // Get factory address
        let factory = get_factory(&env).ok_or(RouterError::PairNotFound)?;

        // Get pair address from factory (factory also sorts tokens internally)
        let pair_address = get_pair_address(&env, &factory, &token_0, &token_1)?;

        // Get pair contract client and current reserves
        // Reserves are returned as (reserve_0, reserve_1) matching (token_0, token_1)
        let pair_client = PairClient::new(&env, &pair_address);
        let (reserve_0, reserve_1, _) = pair_client.get_reserves();

        // Calculate optimal deposit amounts preserving pool ratio
        let (amount_0, amount_1) = compute_optimal_amounts(
            amount_0_desired,
            amount_1_desired,
            amount_0_min,
            amount_1_min,
            reserve_0,
            reserve_1,
        )?;

        // The user must provide authorization for token transfers
        to.require_auth();

        // Transfer tokens from 'to' to the pair contract (in canonical order)
        TokenClient::new(&env, &token_0).transfer(&to, &pair_address, &amount_0);
        TokenClient::new(&env, &token_1).transfer(&to, &pair_address, &amount_1);

        // Mint LP tokens to the recipient
        let liquidity = pair_client.mint(&to);

        // Return amounts in caller's original token order
        let (ret_amount_a, ret_amount_b) = if swapped {
            (amount_1, amount_0)
        } else {
            (amount_0, amount_1)
        };

        Ok((ret_amount_a, ret_amount_b, liquidity))
    }

    /// Commits to a future swap by storing a hash of the intended parameters.
    ///
    /// The caller computes the hash off-chain as:
    /// sha256(sender || token_in || token_out || amount_in || min_out || nonce || salt)
    /// where addresses are XDR-encoded and integers are big-endian.
    ///
    /// The commit records the current ledger sequence so that reveal_swap can
    /// enforce a minimum delay, preventing front-running by MEV searchers who
    /// observe the mempool.
    ///
    /// Bounded map with expiry (issue 389):
    /// - At most max_commits live commits exist globally (default 32, see
    ///   coralswap-shared). A new sender while at capacity fails with
    ///   TooManyCommits. Overwriting your own pending commit does not change
    ///   the count. Overwriting an expired commit reuses its slot.
    /// - Commits expire after expiry_ledgers (default 1000). Expired commits
    ///   are treated as absent on reveal (CommitExpired) and may be pruned.
    ///
    /// # Arguments
    /// * sender - Address that will later call reveal_swap
    /// * hash   - 32-byte SHA-256 commitment to the swap parameters
    pub fn commit_swap(env: Env, sender: Address, hash: BytesN<32>) -> Result<(), RouterError> {
        sender.require_auth();
        let config = get_commit_config(&env);
        let existing = get_commit(&env, &sender);
        let now = env.ledger().sequence();
        let is_live = match &existing {
            Some(e) => now <= e.ledger.saturating_add(config.expiry_ledgers),
            None => false,
        };
        if existing.is_none() || !is_live {
            // New slot (or reuse of an expired slot): enforce the global bound.
            let count = get_commit_count(&env);
            if count >= config.max_commits {
                return Err(RouterError::TooManyCommits);
            }
            if existing.is_none() {
                set_commit_count(&env, count.saturating_add(1));
            }
        }
        set_commit(&env, &sender, &CommitEntry { hash, ledger: now });
        env.storage().instance().extend_ttl(TTL_THRESHOLD, TTL_EXTEND_TO);
        Ok(())
    }

    /// Sets the commit-reveal bounds (issue 389). Governance-only: requires
    /// authorization from the factory fee_to_setter, mirroring sweep().
    pub fn set_commit_config(
        env: Env,
        max_commits: u32,
        expiry_ledgers: u32,
    ) -> Result<(), RouterError> {
        let factory = get_factory(&env).ok_or(RouterError::PairNotFound)?;
        let governance =
            FactoryClient::new(&env, &factory).fee_to_setter().ok_or(RouterError::InternalError)?;
        governance.require_auth();
        if max_commits == 0
            || max_commits > coralswap_shared::MAX_COMMITS_HARD_CAP
            || expiry_ledgers == 0
            || expiry_ledgers > coralswap_shared::COMMIT_EXPIRY_HARD_CAP
        {
            return Err(RouterError::InvalidCommitConfig);
        }
        set_commit_config(&env, &RouterCommitConfig { max_commits, expiry_ledgers });
        env.storage().instance().extend_ttl(TTL_THRESHOLD, TTL_EXTEND_TO);
        Ok(())
    }

    /// Returns the stored commit for sender, if any. Expiry is not evaluated
    /// here; `reveal_swap` and `prune_expired_commit` check it.
    pub fn get_commit(env: Env, sender: Address) -> Option<CommitEntry> {
        get_commit(&env, &sender)
    }

    /// Returns the number of occupied commit slots. Slots of expired commits
    /// are still counted until they are revealed against or pruned.
    pub fn get_commit_count(env: Env) -> u32 {
        get_commit_count(&env)
    }

    /// Returns the active commit-reveal bounds.
    pub fn get_commit_config(env: Env) -> RouterCommitConfig {
        get_commit_config(&env)
    }

    /// Evicts sender commit if it is expired. Returns true when a slot was
    /// freed, false otherwise. Anyone may call; no auth required.
    pub fn prune_expired_commit(env: Env, sender: Address) -> bool {
        let config = get_commit_config(&env);
        if let Some(entry) = get_commit(&env, &sender) {
            let now = env.ledger().sequence();
            if now > entry.ledger.saturating_add(config.expiry_ledgers) {
                clear_commit(&env, &sender);
                let count = get_commit_count(&env);
                set_commit_count(&env, count.saturating_sub(1));
                env.storage().instance().extend_ttl(TTL_THRESHOLD, TTL_EXTEND_TO);
                return true;
            }
        }
        false
    }

    /// Reveals a previously committed swap and executes it atomically.
    ///
    /// Validation steps (in order, issue 389 adds expiry):
    /// 1. A commit must exist for `sender` — prevents reveals without a prior commit.
    /// 2. At least one full ledger must have elapsed since the commit — enforces the
    ///    MEV-resistant delay window; the hash was already on-chain before searchers
    ///    could act on the revealed parameters.
    /// 3. The nonce must not have been used before — prevents replay attacks where
    ///    the same signed commitment is resubmitted.
    /// 4. `sha256(sender || token_in || token_out || amount_in || min_out || nonce || salt)`
    ///    must match the stored hash — authenticates the payload.
    ///
    /// On success the commit is deleted, the nonce is marked used, and the swap is
    /// routed through the best available path with `u64::MAX` as the deadline (timing
    /// was already enforced by the commit-reveal window).
    ///
    /// # Arguments
    /// * `sender`   - Must match the original committer
    /// * `token_in` / `token_out` - The swap pair
    /// * `amount_in` - Exact input amount
    /// * `min_out`   - Minimum acceptable output (slippage guard)
    /// * `nonce`     - Caller-chosen unique value; reuse is rejected
    /// * `salt`      - Random 32-byte value included in the commitment
    pub fn reveal_swap(
        env: Env,
        sender: Address,
        token_in: Address,
        token_out: Address,
        amount_in: i128,
        min_out: i128,
        nonce: u64,
        salt: BytesN<32>,
    ) -> Result<i128, RouterError> {
        let entry = get_commit(&env, &sender).ok_or(RouterError::CommitNotFound)?;
        let config = get_commit_config(&env);
        let now = env.ledger().sequence();

        if now <= entry.ledger {
            return Err(RouterError::CommitRevealTooEarly);
        }
        // Expiry (issue 389): commits older than ledger + expiry are dead.
        // Free the slot so the global bound does not leak.
        if now > entry.ledger.saturating_add(config.expiry_ledgers) {
            clear_commit(&env, &sender);
            let count = get_commit_count(&env);
            set_commit_count(&env, count.saturating_sub(1));
            env.storage().instance().extend_ttl(TTL_THRESHOLD, TTL_EXTEND_TO);
            return Err(RouterError::CommitExpired);
        }

        if is_nonce_used(&env, &sender, nonce) {
            return Err(RouterError::NonceAlreadyUsed);
        }

        let computed = compute_swap_hash(
            &env, &sender, &token_in, &token_out, amount_in, min_out, nonce, &salt,
        );
        if computed != entry.hash {
            return Err(RouterError::CommitHashMismatch);
        }

        clear_commit(&env, &sender);
        let count = get_commit_count(&env);
        set_commit_count(&env, count.saturating_sub(1));
        set_nonce_used(&env, &sender, nonce);
        env.storage().instance().extend_ttl(TTL_THRESHOLD, TTL_EXTEND_TO);

        let (path, _) = Self::get_best_path(env.clone(), token_in, token_out, amount_in)?;
        Self::swap_exact_tokens_multi_hop(env, path, amount_in, min_out, sender, u64::MAX, None)
    }

    /// Removes liquidity from a token pair.
    ///
    /// Transfers `liquidity` LP tokens to the pair, burns them, and sends the
    /// underlying tokens to `to`. Returns `(amount_a, amount_b)`.
    ///
    /// # Arguments
    /// * `token_a` - First token address
    /// * `token_b` - Second token address
    /// * `liquidity` - Amount of LP tokens to burn
    /// * `amount_a_min` - Minimum amount of token_a to receive
    /// * `amount_b_min` - Minimum amount of token_b to receive
    /// * `to` - Recipient of underlying tokens
    /// * `deadline` - Unix timestamp after which the transaction will revert
    /// * `deadline_ledger` - Optional ledger sequence after which the transaction will
    ///   revert; `None` disables the ledger bound (see `check_deadline`)
    pub fn remove_liquidity(
        env: Env,
        token_a: Address,
        token_b: Address,
        liquidity: i128,
        amount_a_min: i128,
        amount_b_min: i128,
        to: Address,
        deadline: u64,
        deadline_ledger: Option<u32>,
    ) -> Result<(i128, i128), RouterError> {
        check_deadline(&env, deadline, deadline_ledger)?;

        // Check for non-zero liquidity
        if liquidity <= 0 {
            return Err(RouterError::ZeroAmount);
        }

        // Check for identical tokens
        if token_a == token_b {
            return Err(RouterError::IdenticalTokens);
        }

        // Issue #357: Ensure tokens are in canonical order.
        // Pair's burn() returns amounts in canonical order (token_0, token_1).
        let (token_0, token_1) = sort_tokens(&token_a, &token_b)?;
        let swapped = token_0 != token_a;
        let (amount_0_min, amount_1_min) = if swapped {
            (amount_b_min, amount_a_min)
        } else {
            (amount_a_min, amount_b_min)
        };

        // Get factory address
        let factory = get_factory(&env).ok_or(RouterError::PairNotFound)?;

        // Get pair address
        let pair_address = get_pair_address(&env, &factory, &token_0, &token_1)?;

        // Get pair contract client
        let pair_client = PairClient::new(&env, &pair_address);

        // The user must provide authorization for the Router to transfer LP tokens
        to.require_auth();

        // Stage the LP with the pair, attributed to `to` (issue #363). The pair
        // burns exactly this amount for `to`, so a concurrent withdrawal by
        // another user can never consume it (and vice versa).
        pair_client.deposit_lp(&to, &liquidity);

        // Call Pair::burn(to) - this will burn LP tokens from the pair and transfer underlying tokens
        // Returns amounts in canonical order: (amount_0, amount_1)
        let (amount_0, amount_1) = pair_client.burn(&to);

        // Enforce minimum output amounts (in canonical order)
        if amount_0 < amount_0_min || amount_1 < amount_1_min {
            return Err(RouterError::InsufficientOutputAmount);
        }

        // Return amounts in caller's original token order
        let (ret_amount_a, ret_amount_b) = if swapped {
            (amount_1, amount_0)
        } else {
            (amount_0, amount_1)
        };

        Ok((ret_amount_a, ret_amount_b))
    }
}
