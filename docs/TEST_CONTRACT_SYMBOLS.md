# Test-contract symbols and WASM isolation

Soroban `#[contractimpl]` exports a method under its **method name**, without a
Rust type or module prefix. Two implementations of `on_flash_loan` in one WASM
artifact collide even if their structs live in different Rust modules. Moving
them into `mod mock_flash_receiver` and `mod malicious_flash_receiver` does not
fix the linker error. See the [SDK export rules](https://docs.rs/soroban-sdk/27.0.2/soroban_sdk/attr.contractimpl.html).

## Convention

- Name each double's crate `coralswap-<purpose>-receiver` and use its Rust crate
  path as the namespace, e.g.
  `coralswap_mock_flash_receiver::MockFlashReceiver` and
  `coralswap_malicious_flash_receiver::MaliciousFlashReceiver`.
- A deployable double has its own `contracts/<purpose>_receiver` workspace
  member and `cdylib` target. Keep **one exported contract per WASM artifact**.
  The mock receiver exports `on_flash_loan` in `coralswap_mock_flash_receiver.wasm`.
- The namespace is the artifact/crate boundary, **not** part of the wire symbol.
  Keep the callback named `on_flash_loan`: the pair's `FlashReceiverClient` calls
  that ABI. Do not invent `mock_flash_receiver::on_flash_loan` as a wire name or
  rename a callback to make the linker happy.
- Shared interfaces use `#[contractclient]` traits; they do not
  contain a `#[contractimpl]` implementation. Depend on interfaces rather than
  pulling other exported contract implementations into a double's WASM graph.
- A native-only double is an `rlib` with
  `#![cfg(not(target_family = "wasm"))]`. Register it with `env.register` in
  native tests. **`rlib` alone is not an export guard**: an rlib's implementation
  can still be linked into a downstream cdylib. The explicit target gate keeps
  native stubs out of that graph. If such a double later needs a WASM artifact,
  give it an isolated deployable crate and retain its behavioral tests.
- Import doubles in `[dev-dependencies]` of the tested contract. Do not combine
  multiple exported receiver implementations in one crate, including in nested
  modules or feature-gated variants that could be enabled together.

The honest mock remains deployable; the adversarial receiver is native-only.
`contracts/pair/src/test/flash_loan.rs` registers both and covers repayment,
underpayment, callback failure and reentrancy. Their callback ABI and test
coverage must remain intact when adding new doubles.

## Build-order regression gate

Run with the pinned Rust toolchain and Python 3.11+:

```sh
python3 -m unittest discover -s scripts -p 'test_stress_contract_builds.py'
python3 scripts/stress_contract_builds.py
# Replay the exact package orders from a failed CI log:
python3 scripts/stress_contract_builds.py --seed 338
```

The script discovers **every workspace cdylib** using `cargo metadata`, so a new
deployable contract automatically participates. It builds each target twice in
random order for `wasm32v1-none`, printing the seed and both orders. Each build
is preceded by a package-scoped clean in `target/contract-build-stress`; the
second pass must relink rather than reuse a cached root artifact. Dependencies
can remain cached. Cargo/linker failures fail the job immediately. This catches
collisions within an artifact, not identical ABI names in separate WASM files
(which are valid).

The current workspace exposes seven cdylib targets: factory, pair, lp-token,
router, mock-flash-receiver, shared and flash-receiver-interface. The last two
have no exported contract implementation, but their existing cdylib targets
still participate; the script does not maintain a hand-written exclusion list.
The CI stress job also builds the normal WASM fixtures and runs the native
pair flash-loan tests. The malicious receiver is an rlib-only native stub and
is exercised through those native tests instead.
