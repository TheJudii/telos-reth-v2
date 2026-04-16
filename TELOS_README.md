# Telos Reth v2

A fork of [reth](https://github.com/paradigmxyz/reth) v2.0.0 adapted for the
Telos EVM network (chain IDs 40/41).

> **Status:** in-progress port. Several v1 features are defined but not yet
> wired in; see [Not yet implemented](#not-yet-implemented) below. For the
> complete audit against v1, see [`AUDIT.md`](./AUDIT.md).

## What this fork does

- **Custom chainspecs** for Telos mainnet (chain 40), testnet (chain 41), and
  their post-rewind `_base` variants
  (`telos-mainnet`, `telos-testnet`, `telos-mainnet-base`, `telos-testnet-base`).
  Genesis hashes are asserted to match the v1 constants:
  - `telos-mainnet` → `0x36fe7024b760365e3970b7b403e161811c1e626edd68460272fcdfa276272563`
  - `telos-testnet` → `0xb25034033c9ca7a40e879ddcc29cf69071a22df06688b5fe8cc2d68b4e0528f9`
  - `telos-mainnet-base` → `0x757720a8e51c63ef1d4f907d6569dacaa965e91c2661345902de18af11f81063`
  - `telos-testnet-base` → `0xa6da3143bdeab454a923ac47589700ebe75d734f26e1f9201caa9b7268045d02`
- **Engine validator** that preserves the consensus client's block hash and
  strips `base_fee_per_gas` from legacy payloads.
- **`trust_consensus` mode** that disables reth's independent verification in
  favor of nodeos consensus. Auto-enabled on Telos chains; opt-out via
  `--telos.trust_consensus=false`. Refused on non-Telos chains.

## What is disabled when `trust_consensus` is on

- State-root verification
- Receipt-root verification
- Block `gas_used` check
- Parent-number continuity, **except** at the genesis→first-block boundary
  (genesis (block 0) is allowed to be followed by a non-sequential block number
  because the Telos EVM does not start at block 1; after block 1, continuity is
  re-enforced)
- EVM transaction execution during historical sync
- Pipeline Merkle stage verification
- Static-file tx-number validation

The node is effectively trusting the consensus client (`nodeos`) as the sole
source of execution truth. Do not enable on chains you do not control.

## Not yet implemented

Scaffolding is present in the repo but not wired into the hot path. These
items are tracked with `TODO(PR 2)` comments at the integration sites:

- **Transaction forwarding to nodeos via `eosio.evm`.** `TelosClient` in
  `crates/telos/rpc/src/telos_client.rs` defines `send_to_telos`, but no RPC
  path calls it. V1 overrides `send_raw_transaction` on a custom `TelosEthApi`
  (see `crates/telos/rpc/src/eth/transaction.rs:38` in v1); that override has
  not yet been ported.
- **State-diff comparison between revm and the Telos EVM contract.**
  `compare_state_diffs` in `crates/telos/rpc-engine-api/src/compare.rs` has a
  signature regression — it takes `State<DB>` and reads post-commit state
  instead of v1's `HashMap<Address, TransitionAccount>` pre-commit diff, and
  it is never called from the block executor.
- **Telos-aware per-tx gas pricing** (`TelosBlockExtension` threading).
  `TelosBlockExtension`, `TelosTxEnv`, `GasPrice`, `Revision` are defined in
  `crates/telos/primitives-traits/src/lib.rs` but never referenced. In v1 they
  are threaded through `fill_tx_env` / `tx_env`
  (see `crates/evm/src/lib.rs:118` in v1).

## Roadmap

- **PR 1 (this PR):** safe foundational fixes — correct genesis, `_base`
  chainspecs, narrow `trust_consensus` bypasses, gate the flag by chain ID,
  move signer key off the CLI, mark dead code.
- **PR 2:** wire `TelosClient`, `compare_state_diffs`, and `TelosBlockExtension`
  into the EVM execution path. Requires trait-bound changes in
  `crates/engine/` and `crates/ethereum/evm/`.
- **PR 3:** port the integration tests from v1.

## Telos crates

| Crate | Description |
|-------|-------------|
| `telos-reth` | Binary — the Telos reth node executable |
| `reth-node-telos` | Node configuration and builder types |
| `reth-telos-primitives-traits` | Telos-specific primitive types (block extensions, gas prices) — **see "Not yet implemented"** |
| `reth-telos-rpc` | TelosClient for native chain interaction — **see "Not yet implemented"** |
| `reth-telos-rpc-engine-api` | Engine API extensions (state diffs, extra fields) — **see "Not yet implemented"** |

## Building

```bash
cargo build --release -p telos-reth --bin telos-reth
```

## Running

```bash
# Telos mainnet (trust_consensus auto-enables).
./target/release/telos-reth node --chain telos-mainnet

# Post-rewind mainnet snapshot.
./target/release/telos-reth node --chain telos-mainnet-base

# Opt out of trust_consensus on a Telos chain (e.g. for testing verification).
./target/release/telos-reth node --chain telos-mainnet --telos.trust_consensus=false

# With Telos native forwarding. Prefer --telos.signer_key_file over --telos.signer_key
# to avoid leaking the key into /proc/<pid>/cmdline.
./target/release/telos-reth node \
  --chain telos-mainnet \
  --telos.telos_endpoint http://localhost:8888 \
  --telos.signer_account rpc.evm \
  --telos.signer_permission active \
  --telos.signer_key_file /etc/telos-reth/signer.key
```

Attempting `./target/release/telos-reth node --chain mainnet --telos.trust_consensus`
will refuse to start, since `trust_consensus` is only valid on Telos chains.

## Database maintenance

To clear the freelist: `mdbx_copy -c $OLD $NEW`. See
[Reth Database Compaction](https://paradigmxyz.github.io/reth/run/troubleshooting.html#database-compaction).
