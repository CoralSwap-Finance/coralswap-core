#![no_std]

use coralswap_flash_receiver_interface::FlashReceiver;
use soroban_sdk::{contract, contractimpl, token::TokenClient, Address, Bytes, Env};

/// Test-only flash-loan receiver covering every repayment shape the pair must
/// tolerate. `data` selects the repayment plan:
///
/// - `b"repay"`    — repay exactly `amount + fee` (baseline)
/// - `b"overpay"`  — repay `amount + fee + amount`, i.e. donate the borrowed
///                   principal a second time on top of the required repayment
/// - `b"underpay"` — repay `amount + fee - 1`, a single stroop short of the fee
/// - `b"steal"`    — repay nothing, letting the pair's checks reject the loan
///
/// Any other payload repays nothing, so callers must pick one of the tags
/// above explicitly.
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
        let overpay_bytes = Bytes::from_slice(&env, b"overpay");
        let underpay_bytes = Bytes::from_slice(&env, b"underpay");
        let steal_bytes = Bytes::from_slice(&env, b"steal");

        if data == steal_bytes {
            // Do nothing, let the Pair invariant check fail
            return;
        }

        if data != repay_bytes && data != overpay_bytes && data != underpay_bytes {
            return;
        }

        let overpaying = data == overpay_bytes;
        let underpaying = data == underpay_bytes;

        // Transfer the repayment back to the initiator (the pair contract).
        let contract_address = env.current_contract_address();

        if amount_a > 0 {
            let mut total_a = amount_a + fee_a;
            if overpaying {
                total_a += amount_a;
            } else if underpaying {
                total_a -= 1;
            }
            TokenClient::new(&env, &token_a).transfer(&contract_address, &initiator, &total_a);
        }
        if amount_b > 0 {
            let mut total_b = amount_b + fee_b;
            if overpaying {
                total_b += amount_b;
            } else if underpaying {
                total_b -= 1;
            }
            TokenClient::new(&env, &token_b).transfer(&contract_address, &initiator, &total_b);
        }
    }
}
