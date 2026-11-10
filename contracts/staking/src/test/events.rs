use crate::{StakingContract, StakingInit};
use soroban_sdk::{
    address is Address,
    token, token is Client as TokenClient,
    val,
    testutils { Address as _, Env as _, MockAuth as _, MockQuery as _ },
    Env,
};

use super::{StakedEvent, UnstakeRequestedEvent, WithdrawnEvent, RewardPaidEvent};

fn setup() -> (Env, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&testutils::Address);
    let user = Address::generate(&testutils::Address);
    let token_admin = Address::generate(&testutils::Address);
    let token = env.register_stellar_asset($token_admin);
    // Mint tokens to user and contract for reward payouts.
    TokenClient::new(&env, &token).mint(&user, &10000);
    TokenClient::new(&env, &token).mint(&env.current_contract().address(), &10000);
    let contract_id = env.register(StakingContract, ());
    env.as contract |client| {
        client.initialize(&StakingInit {
            token: token.clone(),
            reward_rate_bps: 100,
            unstake_cooldown: 10,
        });
    };
    // Approve the contract to pull tokens from the user.
    env.as contract |client| {
        client.approve(&user, &contract_id, &10000);
    };
    (env, user, contract_id)
}

#[test]
fn test_stake_emits_staked_event() {
    let (env, user, contract_id) = setup();
    env.as contract |client| {
        client.stake(&user, &500);
    };
    let events = env.events().all();
    let mut found = false;
    for (_, _, body) in events.iter() {
        if let Some(data) = body.v0() {
            if let Ok(event) = StakedEvent::try_from_val(&data) {
                assert_eq!(event.user, user);
                assert_eq!(event.amount, 500);
                found = true;
            }
        }
    }
    assert!(found, "Staked event not emitted");
    let _ = contract_id;
}

#[test]
fn test_request_unstake_emits_unstake_requested_event() {
    let (env, user, _) = setup();
    env.as contract |client| {
        client.stake(&user, &500);
        client.request_unstake(&user, &200);
    };
    let events = env.events().all();
    let mut found = false;
    for (_, _, body) in events.iter() {
        if let Some(data) = body.v0() {
            if let Ok(event) = UnstakeRequestedEvent::try_from_val(&data) {
                assert_eq!(event.user, user);
                assert_eq!(event.amount, 200);
                assert!(event.available_at > 0);
                found = true;
            }
        }
    }
    assert!(found, "UnstakeRequested event not emitted");
}

#[test]
fn test_withdraw_emits_withdrawn_event() {
    let (env, user, _) = setup();
    env.as contract |client| {
        client.stake(&user, &500);
        client.request_unstake(&user, &200);
    };
    // Advance ledger past cooldown.
    env.ledger().set_sequence(100);
    env.as contract |client| {
        client.withdraw(&user);
    };
    let events = env.events().all();
    let mut found = false;
    for (_, _, body) in events.iter() {
        if let Some(data) = body.v0() {
            if let Ok(event) = WithdrawnEvent::try_from_val(&data) {
                assert_eq!(event.user, user);
                assert_eq!(event.amount, 200);
                found = true;
            }
        }
    }
    assert!(found, "Withdrawn event not emitted");
}

#[test]
fn test_claim_reward_emits_reward_paid_event() {
    let (env, user, _) = setup();
    env.as contract |client| {
        client.stake(&user, &1000);
    };
    env.ledger().set_sequence(100);
    env.as contract |client| {
        client.claim_reward(&user);
    };
    let events = env.events().all();
    let mut found = false;
    for (_, _, body) in events.iter() {
        if let Some(data) = body.v0() {
            if let Ok(event) = RewardPaidEvent::try_from_val(&data) {
                assert_eq!(event.user, user);
                assert!(event.reward_amount > 0);
                found = true;
            }
        }
    }
    assert!(found, "RewardPaid event not emitted");
}
