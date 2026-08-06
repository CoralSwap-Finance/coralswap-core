#![no_std]

use coralswap_flash_receiver_interface::FlashReceiver;
use soroban_sdk::{contract, contractimpl, token::TokenClient, Address, Bytes, Env};

#[contract]
pub struct MockFlashReceiver;

#[contractimpl]
impl FlashReceiver for MockFlashReceiver {
    fn on_flash_loan(
        env: Env,
        initiator: Address,
        token_a: Address,
        token_b: Address,
        amount_a: i128,
        amount_b: i128,
        fee_a: i128,
        fee_b: i128,
        data: Bytes,
    ) {
        let repay_bytes = Bytes::from_slice(&env, b"repay");
        let steal_bytes = Bytes::from_slice(&env, b"steal");

        if data == repay_bytes {
            // Transfer back amount + fee to the initiator
            let contract_address = env.current_contract_address();

            if amount_a > 0 {
                let total_a = amount_a + fee_a;
                TokenClient::new(&env, &token_a).transfer(&contract_address, &initiator, &total_a);
            }
            if amount_b > 0 {
                let total_b = amount_b + fee_b;
                TokenClient::new(&env, &token_b).transfer(&contract_address, &initiator, &total_b);
            }
        } else if data == steal_bytes {
            // Do nothing, let the Pair invariant check fail
        }
    }
}
