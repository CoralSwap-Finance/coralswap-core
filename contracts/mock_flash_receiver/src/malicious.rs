#![no_std]

use coralswap_flash_receiver_interface::FlashReceiver;
use coralswap_pair::PairClient;
use soroban_sdk::{contract, contractimpl, Address, Bytes, Env};

/// Adversarial flash-loan receiver used in reentrancy tests.
///
/// `data` selects the attack vector:
/// - `b"attack_swap"` — re-enter the pair via `swap()` during the callback
/// - `b"attack_flash"` — nest another `flash_loan()` during the callback
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

        if data == attack_swap {
            let to = env.current_contract_address();
            pair.try_swap(&0, &1, &to).unwrap();
        } else if data == attack_flash {
            let receiver = env.current_contract_address();
            let nested = Bytes::from_slice(&env, b"nested");
            pair.try_flash_loan(&receiver, &amount_a, &amount_b, &nested).unwrap();
        }
    }
}
