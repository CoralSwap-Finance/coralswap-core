#![no_std]

use coralswap_flash_receiver_interface::FlashReceiver;
use coralswap_pair::PairClient;
use soroban_sdk::{contract, contracterror, contractimpl, Address, Bytes, Env};

#[contracterror]
pub enum ReceiverError {
    CallbackFailed = 1,
}

/// Adversarial flash-loan receiver used in reentrancy tests.
///
/// `data` selects the attack vector:
/// - `b"attack_swap"` — re-enter the pair via `swap()` during the callback
/// - `b"attack_flash"` — nest another `flash_loan()` during the callback
/// - `b"attack_fail"` — revert inside the callback without borrowing anything
///
/// Only ever registered natively (`env.register`) in unit tests,
/// never deployed as a real wasm contract — this crate has no `cdylib`
/// target, so it can't collide with `MockFlashReceiver`'s exported
/// `on_flash_loan` symbol at the wasm level even if both ended up in the
/// same build graph.
#[contract]
pub struct MaliciousFlashReceiver;

#[contractimpl]
impl FlashReceiver for MaliciousFlashReceiver {
    fn on_flash_loan(
        env: Env,
        initiator: Address,
        _token_a: Address,
        _token_b: Address,
        amount_a: i128,
        amount_b: i128,
        _fee_a: i128,
        _fee_b: i128,
        data: Bytes,
    ) {
        let pair = PairClient::new(&env, &initiator);
        let attack_swap = Bytes::from_slice(&env, b"attack_swap");
        let attack_flash = Bytes::from_slice(&env, b"attack_flash");
        let attack_fail = Bytes::from_slice(&env, b"attack_fail");

        if data == attack_fail {
            // Revert the callback itself; the pair must propagate this as
            // `FlashCallbackFailed` instead of treating the loan as repaid.
            env.panic_with_error(ReceiverError::CallbackFailed);
        } else if data == attack_swap {
            let to = env.current_contract_address();
            // The reentrant swap is expected to fail (pair holds the
            // reentrancy lock); the inner contract error is deliberately
            // discarded, only invocation failures panic here.
            let _ = pair.try_swap(&0, &1, &to).unwrap();
        } else if data == attack_flash {
            let receiver = env.current_contract_address();
            let nested = Bytes::from_slice(&env, b"nested");
            let _ = pair.try_flash_loan(&receiver, &amount_a, &amount_b, &nested).unwrap();
        }
    }
}
