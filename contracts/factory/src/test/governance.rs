//! Quorum math for every multisig-gated governance function (issue #364).
//!
//! Quorum is a strict majority of the *registered* signer set: `> n / 2`, i.e.
//! `n / 2 + 1`. For each of `pause`, `unpause`, `freeze_pair`, `unfreeze_pair`,
//! `propose_upgrade` and `cancel_upgrade` these tests cover: the full set, a
//! majority, exactly the threshold, one below the threshold, duplicated
//! signatures, and unregistered addresses used as padding.

#![cfg(test)]

use soroban_sdk::{testutils::Address as _, Address, Bytes, BytesN, Env, Vec};

use crate::{errors::FactoryError, governance::quorum_threshold, Factory, FactoryClient};

/// Every governance function that is gated by `verify_multisig`.
#[derive(Clone, Copy, Debug)]
enum Gov {
    Pause,
    Unpause,
    FreezePair,
    UnfreezePair,
    ProposeUpgrade,
    CancelUpgrade,
}

const ALL: [Gov; 6] = [
    Gov::Pause,
    Gov::Unpause,
    Gov::FreezePair,
    Gov::UnfreezePair,
    Gov::ProposeUpgrade,
    Gov::CancelUpgrade,
];

/// A freshly deployed factory with `n` registered signers.
fn fresh(n: u32) -> (Env, FactoryClient<'static>, Vec<Address>) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(Factory, ());
    let client = FactoryClient::new(&env, &id);

    let mut signers = Vec::new(&env);
    for _ in 0..n {
        signers.push_back(Address::generate(&env));
    }
    let pair_wasm = env.deployer().upload_contract_wasm(Bytes::new(&env));
    let lp_wasm = env.deployer().upload_contract_wasm(Bytes::new(&env));
    client.initialize(&signers, &pair_wasm, &lp_wasm, &Address::generate(&env));
    (env, client, signers)
}

/// Calls `gov` with `presented` and reports the outcome.
///
/// `NoPendingUpgrade` from `cancel_upgrade` is mapped to `Ok`: it is raised
/// *after* the quorum check passed, so it proves the quorum was accepted
/// without needing a proposal to exist.
fn call(
    env: &Env,
    c: &FactoryClient,
    gov: Gov,
    presented: &Vec<Address>,
) -> Result<(), FactoryError> {
    let pair = Address::generate(env);
    let hash = BytesN::from_array(env, &[7u8; 32]);
    let r = match gov {
        Gov::Pause => c.try_pause(presented).map(|_| ()),
        Gov::Unpause => c.try_unpause(presented).map(|_| ()),
        Gov::FreezePair => c.try_freeze_pair(presented, &pair).map(|_| ()),
        Gov::UnfreezePair => c.try_unfreeze_pair(presented, &pair).map(|_| ()),
        Gov::ProposeUpgrade => c.try_propose_upgrade(presented, &hash).map(|_| ()),
        Gov::CancelUpgrade => c.try_cancel_upgrade(presented).map(|_| ()),
    };
    match r {
        Ok(()) => Ok(()),
        Err(Ok(FactoryError::NoPendingUpgrade)) => Ok(()),
        Err(Ok(e)) => Err(e),
        Err(Err(e)) => panic!("{gov:?}: host-level failure {e:?}"),
    }
}

fn first(signers: &Vec<Address>, k: u32) -> Vec<Address> {
    let mut out = Vec::new(signers.env());
    for i in 0..k {
        out.push_back(signers.get(i).unwrap());
    }
    out
}

// ── Pure quorum math ────────────────────────────────────────────────────────

#[test]
fn test_quorum_threshold_table() {
    let expected =
        [(1, 1), (2, 2), (3, 2), (4, 3), (5, 3), (6, 4), (7, 4), (8, 5), (9, 5), (10, 6)];
    for (n, q) in expected {
        assert_eq!(quorum_threshold(n), q, "quorum for n = {n}");
    }
}

#[test]
fn test_quorum_is_strict_majority_for_every_size() {
    for n in 1..=10u32 {
        let q = quorum_threshold(n);
        assert!(q <= n, "n = {n}: quorum must be reachable");
        assert!(2 * q > n, "n = {n}: quorum {q} must be a strict majority");
        assert!(2 * (q - 1) <= n, "n = {n}: quorum {q} must be minimal");
    }
}

/// Two disjoint groups can never both reach quorum, so a governance action and
/// its opposite (pause/unpause, propose/cancel) cannot be issued at once.
#[test]
fn test_two_disjoint_groups_never_both_reach_quorum() {
    for n in 1..=10u32 {
        let q = quorum_threshold(n);
        assert!(q + q > n, "n = {n}: two quorums must overlap");
    }
}

/// `n -> n - 1`: as signers are revoked, quorum never drops to zero and never
/// exceeds the remaining set, so a governance address cannot hold a veto forever
/// nor can the set be left unable to act.
#[test]
fn test_quorum_when_signers_are_revoked_one_by_one() {
    let mut n = 10u32;
    while n > 1 {
        let q = quorum_threshold(n);
        let q_after = quorum_threshold(n - 1);
        assert!(q_after >= 1, "quorum never reaches zero");
        assert!(q_after < n, "quorum stays reachable after revocation");
        assert!(q_after <= q, "revocation never raises quorum");
        assert!(q - q_after <= 1, "quorum moves by at most one per revocation");
        n -= 1;
    }
    assert_eq!(quorum_threshold(1), 1, "a lone signer is the whole quorum");
}

/// The exact set a veto would need: with `n` signers, `n - q` non-cooperating
/// signers cannot block, `n - q + 1` can.
#[test]
fn test_veto_size_matches_quorum() {
    for n in 1..=10u32 {
        let q = quorum_threshold(n);
        let blockers_needed = n - q + 1;
        assert!(blockers_needed >= 1);
        assert!(
            n - (blockers_needed - 1) >= q,
            "n = {n}: {} blockers can not veto",
            blockers_needed - 1
        );
        assert!(n - blockers_needed < q, "n = {n}: {blockers_needed} blockers do veto");
    }
}

// ── Each governance function, each signer-set size ──────────────────────────

#[test]
fn test_full_set_reaches_quorum() {
    for gov in ALL {
        for n in 1..=10 {
            let (env, c, signers) = fresh(n);
            assert_eq!(call(&env, &c, gov, &signers), Ok(()), "{gov:?}, n = {n}");
        }
    }
}

#[test]
fn test_exactly_threshold_reaches_quorum() {
    for gov in ALL {
        for n in 1..=10 {
            let (env, c, signers) = fresh(n);
            let presented = first(&signers, quorum_threshold(n));
            assert_eq!(call(&env, &c, gov, &presented), Ok(()), "{gov:?}, n = {n}");
        }
    }
}

#[test]
fn test_majority_reaches_quorum() {
    for gov in ALL {
        for n in 1..=10 {
            let (env, c, signers) = fresh(n);
            let presented = first(&signers, n / 2 + 1);
            assert_eq!(call(&env, &c, gov, &presented), Ok(()), "{gov:?}, n = {n}");
        }
    }
}

#[test]
fn test_below_threshold_is_rejected() {
    for gov in ALL {
        for n in 1..=10 {
            let (env, c, signers) = fresh(n);
            let presented = first(&signers, quorum_threshold(n) - 1);
            assert_eq!(
                call(&env, &c, gov, &presented),
                Err(FactoryError::InsufficientSignatures),
                "{gov:?}, n = {n}"
            );
        }
    }
}

/// Exactly half is not a majority (the old `ceil(n/2)` scale accepted it).
#[test]
fn test_exactly_half_is_rejected_for_even_sets() {
    for gov in ALL {
        for n in [2u32, 4, 6, 8, 10] {
            let (env, c, signers) = fresh(n);
            let presented = first(&signers, n / 2);
            assert_eq!(
                call(&env, &c, gov, &presented),
                Err(FactoryError::InsufficientSignatures),
                "{gov:?}, n = {n}"
            );
        }
    }
}

#[test]
fn test_empty_signer_list_is_rejected() {
    for gov in ALL {
        let (env, c, _signers) = fresh(3);
        assert_eq!(
            call(&env, &c, gov, &Vec::new(&env)),
            Err(FactoryError::InsufficientSignatures),
            "{gov:?}"
        );
    }
}

// ── Duplicate signatures ────────────────────────────────────────────────────

#[test]
fn test_duplicate_signer_is_rejected() {
    for gov in ALL {
        let (env, c, signers) = fresh(5);
        let s0 = signers.get(0).unwrap();
        // One signer repeated up to the threshold must not count three times.
        let presented = Vec::from_array(&env, [s0.clone(), s0.clone(), s0]);
        assert_eq!(call(&env, &c, gov, &presented), Err(FactoryError::DuplicateSigner), "{gov:?}");
    }
}

#[test]
fn test_duplicate_does_not_pad_a_partial_quorum() {
    for gov in ALL {
        let (env, c, signers) = fresh(5); // quorum 3
        let s0 = signers.get(0).unwrap();
        let s1 = signers.get(1).unwrap();
        let presented = Vec::from_array(&env, [s0.clone(), s1, s0]);
        assert_eq!(call(&env, &c, gov, &presented), Err(FactoryError::DuplicateSigner), "{gov:?}");
    }
}

#[test]
fn test_duplicate_alongside_full_quorum_is_still_rejected() {
    for gov in ALL {
        let (env, c, signers) = fresh(3); // quorum 2
        let mut presented = first(&signers, 2);
        presented.push_back(signers.get(0).unwrap());
        assert_eq!(call(&env, &c, gov, &presented), Err(FactoryError::DuplicateSigner), "{gov:?}");
    }
}

// ── Unregistered addresses ──────────────────────────────────────────────────

/// Unregistered addresses must not count toward quorum. Before this change,
/// `propose_upgrade` / `cancel_upgrade` accepted *any* addresses, and
/// `pause` & co. accepted one registered signer padded with strangers.
#[test]
fn test_unregistered_padding_is_rejected() {
    for gov in ALL {
        let (env, c, signers) = fresh(5); // quorum 3
        let presented = Vec::from_array(
            &env,
            [signers.get(0).unwrap(), Address::generate(&env), Address::generate(&env)],
        );
        assert_eq!(call(&env, &c, gov, &presented), Err(FactoryError::Unauthorized), "{gov:?}");
    }
}

#[test]
fn test_only_unregistered_addresses_are_rejected() {
    for gov in ALL {
        let (env, c, _signers) = fresh(3);
        let presented = Vec::from_array(&env, [Address::generate(&env), Address::generate(&env)]);
        assert_eq!(call(&env, &c, gov, &presented), Err(FactoryError::Unauthorized), "{gov:?}");
    }
}

#[test]
fn test_unregistered_extra_beside_full_quorum_is_rejected() {
    for gov in ALL {
        let (env, c, signers) = fresh(3);
        let mut presented = first(&signers, 2);
        presented.push_back(Address::generate(&env));
        assert_eq!(call(&env, &c, gov, &presented), Err(FactoryError::Unauthorized), "{gov:?}");
    }
}

// ── Side effects and real authorization ─────────────────────────────────────

#[test]
fn test_rejected_quorum_leaves_state_unchanged() {
    let (env, c, signers) = fresh(5);
    let below = first(&signers, 2);
    assert_eq!(call(&env, &c, Gov::Pause, &below), Err(FactoryError::InsufficientSignatures));
    assert!(!c.is_paused());

    c.pause(&first(&signers, 3));
    assert!(c.is_paused());
    assert_eq!(call(&env, &c, Gov::Unpause, &below), Err(FactoryError::InsufficientSignatures));
    assert!(c.is_paused());
}

/// A signer that reaches quorum by count but has not actually signed must
/// still be refused: `require_auth` is enforced for each counted signer.
#[test]
fn test_quorum_by_count_without_signatures_fails() {
    let env = Env::default(); // deliberately no mock_all_auths
    let id = env.register(Factory, ());
    let c = FactoryClient::new(&env, &id);
    let signers = Vec::from_array(&env, [Address::generate(&env)]);
    let pair_wasm = env.deployer().upload_contract_wasm(Bytes::new(&env));
    let lp_wasm = env.deployer().upload_contract_wasm(Bytes::new(&env));
    c.initialize(&signers, &pair_wasm, &lp_wasm, &Address::generate(&env));

    assert!(c.try_pause(&signers).is_err(), "unsigned quorum must not pause");
    assert!(!c.is_paused());
}
