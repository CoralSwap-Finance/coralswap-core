use crate::errors::FactoryError;
use crate::storage::{self, GovernanceProposal, ProposalStatus};
use soroban_sdk::{Address, Env, Symbol, Val, Vec};

/// Timelock delay for governance proposals: 72 hours at ~5-second ledger close.
/// 72 * 3_600 / 5 = 51_840 ledgers.  Mirrors `UPGRADE_DELAY_LEDGERS` in
/// `upgrade.rs` so both timelocks give users the same reaction window.
pub const PROPOSAL_DELAY_LEDGERS: u32 = 51_840;

/// Creates a new governance proposal in the `Succeeded` state.
///
/// The multisig quorum check **must** be performed by the caller before
/// invoking this function.  The proposal is immediately in `Succeeded` state
/// because the quorum was satisfied in the same transaction.
///
/// Returns the newly assigned proposal ID.
pub fn create_proposal(
    env: &Env,
    target: Address,
    function: Symbol,
    args: Vec<Val>,
) -> Result<u32, FactoryError> {
    let id = storage::get_proposal_count(env);
    let proposal = GovernanceProposal {
        id,
        target,
        function,
        args,
        proposed_at_ledger: env.ledger().sequence(),
        queued_at_ledger: None,
        status: ProposalStatus::Succeeded,
    };
    storage::set_proposal(env, &proposal);
    storage::set_proposal_count(env, id + 1);
    Ok(id)
}

/// Moves a `Succeeded` proposal into the timelock queue.
///
/// # Errors
/// * `ProposalNotFound`       — no proposal exists for `proposal_id`
/// * `ProposalNotSucceeded`   — proposal is not in the `Succeeded` state
/// * `ProposalAlreadyQueued`  — proposal is already `Queued`
/// * `ProposalAlreadyExecuted`— proposal was already executed
pub fn queue_proposal(env: &Env, proposal_id: u32) -> Result<(), FactoryError> {
    let mut proposal =
        storage::get_proposal(env, proposal_id).ok_or(FactoryError::ProposalNotFound)?;

    match proposal.status {
        ProposalStatus::Queued => return Err(FactoryError::ProposalAlreadyQueued),
        ProposalStatus::Executed => return Err(FactoryError::ProposalAlreadyExecuted),
        ProposalStatus::Succeeded => {}
    }

    let queued_at = env.ledger().sequence();
    proposal.queued_at_ledger = Some(queued_at);
    proposal.status = ProposalStatus::Queued;
    storage::set_proposal(env, &proposal);

    crate::events::FactoryEvents::proposal_queued(env, proposal_id, &proposal.target, queued_at);
    Ok(())
}

/// Executes a `Queued` proposal after the timelock delay has passed.
///
/// Uses `env.invoke_contract` to dispatch the call described by the proposal's
/// `(target, function, args)` payload.
///
/// # Errors
/// * `ProposalNotFound`            — no proposal exists for `proposal_id`
/// * `ProposalNotSucceeded`        — proposal was never queued (still `Succeeded`)
/// * `ProposalTimelockNotExpired`  — timelock has not yet elapsed
/// * `ProposalAlreadyExecuted`     — proposal was already executed
/// * `ProposalExecutionFailed`     — the cross-contract call returned an error
pub fn execute_proposal(env: &Env, proposal_id: u32) -> Result<(), FactoryError> {
    let mut proposal =
        storage::get_proposal(env, proposal_id).ok_or(FactoryError::ProposalNotFound)?;

    match proposal.status {
        ProposalStatus::Succeeded => return Err(FactoryError::ProposalNotSucceeded),
        ProposalStatus::Executed => return Err(FactoryError::ProposalAlreadyExecuted),
        ProposalStatus::Queued => {}
    }

    let queued_at = proposal.queued_at_ledger.ok_or(FactoryError::ProposalNotSucceeded)?;
    let elapsed = env.ledger().sequence().saturating_sub(queued_at);
    if elapsed < PROPOSAL_DELAY_LEDGERS {
        return Err(FactoryError::ProposalTimelockNotExpired);
    }

    // Mark executed before the cross-contract call so that re-entrant
    // attempts to execute the same proposal are blocked.
    proposal.status = ProposalStatus::Executed;
    storage::set_proposal(env, &proposal);

    let ledger = env.ledger().sequence();

    // Dispatch the cross-contract call described by the proposal payload.
    env.invoke_contract::<()>(&proposal.target, &proposal.function, proposal.args.clone());

    crate::events::FactoryEvents::proposal_executed(env, proposal_id, &proposal.target, ledger);
    Ok(())
}
