# Telos Reth v2 Audit Findings

This audit documents gaps between the v2 port (fork of `reth v2.0.0`) and the v1
reference implementation at <https://github.com/telosnetwork/telos-reth>. Findings
are grouped into three buckets:

1. **Deployment blockers** — wrong or missing configuration that prevents the
   node from peering with the existing Telos network.
2. **Too-broad safety bypasses** — places where `trust_consensus` disables more
   than it should.
3. **Dead code masquerading as working features** — types and functions that are
   defined but never wired into the execution path.

## 1. Deployment blockers

### 1.1 Wrong mainnet/testnet genesis

`crates/telos/node/res/telos-mainnet.json` and `telos-testnet.json` are missing
the `extraData` and `stateRoot` fields needed to reproduce the v1 genesis hashes:

- Telos mainnet (chain 40): `0x36fe7024b760365e3970b7b403e161811c1e626edd68460272fcdfa276272563`
- Telos testnet (chain 41): `0xb25034033c9ca7a40e879ddcc29cf69071a22df06688b5fe8cc2d68b4e0528f9`

The v1 reference files live in `crates/chainspec/res/genesis/tevmmainnet.json`
and `tevmtestnet.json`.

### 1.2 Missing `_base` chainspecs

V1 ships `TEVMMAINNET_BASE` and `TEVMTESTNET_BASE` — post-rewind chainspecs with
Frontier activating at a non-zero block number. These are required because the
Telos EVM does not start at block 1.

- Mainnet base genesis hash: `0x757720a8e51c63ef1d4f907d6569dacaa965e91c2661345902de18af11f81063`
- Testnet base genesis hash: `0xa6da3143bdeab454a923ac47589700ebe75d734f26e1f9201caa9b7268045d02`
- Mainnet base Frontier activation block: `180698823`
- Testnet base Frontier activation block: `137430501`

V2 does not ship these specs. Operators cannot start a node from the post-rewind
snapshot without them.

### 1.3 Signer key on CLI

The signer private key is accepted via `--telos.signer_key` (plaintext argv).
Anyone with `/proc/<pid>/cmdline` access can read it. A file-based alternative
is needed for production deployments.

## 2. Too-broad safety bypasses

### 2.1 Parent-number continuity disabled for all blocks

`crates/consensus/common/src/validation.rs:312` disables parent-number
continuity for **every** block when `trust_consensus=true`. The actual Telos
requirement is only that block 0 (genesis) can be followed by a high block
number. After that, sequentiality should still be enforced.

### 2.2 `trust_consensus` defaults to `true`, applied globally

`TelosArgs::trust_consensus` defaults to `true`, and the flag is applied to the
global atomic without checking the chain ID. A user running
`telos-reth --chain mainnet` (Ethereum mainnet, chain 1) silently gets all
verification disabled. The flag must be gated on chain ID.

## 3. Dead code masquerading as working features

### 3.1 `TelosClient` not wired into `eth_sendRawTransaction`

`crates/telos/rpc/src/telos_client.rs` defines `TelosClient::send_to_telos`,
but no RPC path calls it. In v1, `crates/telos/rpc/src/eth/transaction.rs:38`
overrides `send_raw_transaction` on a custom `TelosEthApi` to forward the tx
through `eosio.evm`. V2 has the client, but not the custom `EthApi`.

### 3.2 `compare_state_diffs` signature regression

V2's `crates/telos/rpc-engine-api/src/compare.rs::compare_state_diffs` takes
`State<DB>` and reads post-commit state via `revm_db.basic(...)`. V1 takes
`revm_state_diffs: HashMap<Address, TransitionAccount>` — the pre-commit
transition state — which is what a state-diff comparison actually needs. The
function as written cannot diff; it can only read post-commit state. It is also
never called from the block executor.

### 3.3 `TelosBlockExtension` / `TelosTxEnv` never threaded through

`TelosBlockExtension`, `TelosTxEnv`, `GasPrice`, `Revision` are defined in
`crates/telos/primitives-traits/src/lib.rs` but never referenced. In v1, they
are threaded into the EVM config via `fill_tx_env` / `tx_env` (see
`crates/evm/src/lib.rs:118`). Without that wiring, Telos-aware per-tx gas
pricing does not apply.

## Scope for PR 1

PR 1 addresses items 1.1, 1.2, 1.3, 2.1, 2.2 plus the honest TODO comments for
bucket 3. The actual wiring of `TelosClient`, `compare_state_diffs`, and
`TelosBlockExtension` into the EVM config is reserved for PR 2 — it requires
trait-bound changes in `crates/engine/` and `crates/ethereum/evm/` that are out
of scope for a safe, mechanical fix.

PR 3 ports the integration tests from v1.
