.PHONY: build build-optimized test lint fmt clean deploy-testnet

# Build all contracts (debug profile)
build:
	soroban contract build

# Build all contracts with release profile, optimised for WASM deployment.
# Produces .wasm files in target/wasm32-unknown-unknown/release/ ready for
# on-chain upload.
build-optimized:
	cargo build --release --target wasm32-unknown-unknown

# Run the full workspace test suite
test:
	cargo test

# Run Clippy with zero-warning policy
lint:
	cargo clippy --all-targets -- -D warnings

# Format all source files
fmt:
	cargo fmt --all

# Remove build artefacts
clean:
	cargo clean

# Deploy all contracts to Soroban testnet.
# Requires SOROBAN_RPC_URL and SOROBAN_NETWORK_PASSPHRASE to be set, and
# soroban-cli to be installed.
deploy-testnet: build-optimized
	@echo "Deploying contracts to testnet..."
	soroban contract deploy \
		--wasm target/wasm32-unknown-unknown/release/factory.wasm \
		--network testnet
	soroban contract deploy \
		--wasm target/wasm32-unknown-unknown/release/pair.wasm \
		--network testnet
	soroban contract deploy \
		--wasm target/wasm32-unknown-unknown/release/lp_token.wasm \
		--network testnet
	soroban contract deploy \
		--wasm target/wasm32-unknown-unknown/release/router.wasm \
		--network testnet
