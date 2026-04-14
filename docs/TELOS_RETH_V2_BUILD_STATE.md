# Telos Reth v2 — build_state Mode & Testnet Learnings

## Overview

This document covers the implementation of `build_state` mode for telos-reth v2 (reth 2.0.0) on Telos EVM testnet (chain_id 41), including architecture decisions, code changes, test results, and known issues discovered during development.

## Architecture

### The Problem
The `trust_consensus` mode in telos-reth skips ALL EVM execution and validation, accepting CL-provided block headers as-is. This means block hashes match production perfectly, but **no EVM state is built** — balances, contract code, and storage are all empty. RPC calls like `eth_getBalance`, `eth_getCode`, and `eth_call` return zeros.

### The Solution: `build_state` Mode
A hybrid approach: `trust_consensus=true` for validation bypasses, but state diffs from the Telos native layer are read and applied to reth's state DB. This builds EVM state without executing transactions.

### How Telos State Diffs Work
On Telos, EVM state comes from the native-layer `eosio.evm` contract, NOT from standard EVM transaction execution. The consensus client (CL):
1. Reads native-layer action traces that modify EVM state
2. Packages them as `TelosEngineAPIExtraFields` (account balances, nonces, code, storage)
3. Writes them to `/tmp/telos-extra-fields/{block_hash}.json`
4. Sends them as the second JSON-RPC parameter in `engine_newPayloadV1`

The `compare_state_diffs()` function in `compare.rs` was already implemented to process these diffs, but was **never called from anywhere** in the codebase. The build_state changes wire it into the block processing pipeline.

## Code Changes

### Files Modified

#### 1. `crates/telos/primitives-traits/src/lib.rs`
- Added `BUILD_STATE` global AtomicBool flag
- Added `set_build_state()` and `build_state()` accessor functions

#### 2. `crates/telos/node/src/args.rs`
- Added `--telos.build_state` CLI flag (flag-style boolean, presence = true)

#### 3. `crates/telos/bin/src/main.rs`
- Wired `set_build_state(telos_args.build_state)` at startup

#### 4. `crates/telos/rpc-engine-api/src/lib.rs`
- Added `parse_extra_fields_from_file()` helper function
- Reads and parses CL extra fields JSON files from `/tmp/telos-extra-fields/`

#### 5. `crates/telos/rpc-engine-api/Cargo.toml`
- Added `serde_json` dependency for JSON parsing

#### 6. `crates/engine/tree/Cargo.toml`
- Added `reth-telos-rpc-engine-api` dependency

#### 7. `crates/engine/tree/src/tree/payload_validator.rs` (Main Changes)

**Three-branch execution output (line ~1073):**
```rust
let output = if trust_consensus() && !build_state() {
    // Pure trust_consensus: skip everything, empty output
    drop(executor);
    drop(db);
    BlockExecutionOutput::default()
} else if trust_consensus() && build_state() {
    // build_state: skip tx execution, read state diffs from CL extra fields,
    // apply via compare_state_diffs(), merge transitions
    drop(executor);
    // Read /tmp/telos-extra-fields/{block_hash}.json
    // Call compare_state_diffs(&mut db, ...)
    // db.merge_transitions() + db.take_bundle()
} else {
    // Normal execution path
    executor.finish() + db.merge_transitions() + db.take_bundle()
};
```

**Overlay factory bypass (line ~545):**
When `trust_consensus` is true, skips the lazy overlay construction which walks ancestor blocks through the changeset cache — prohibitively expensive with millions of blocks.

**State root computation skip (line ~680):**
Pre-sets `maybe_state_root` to the header's state root (empty trie root) and wraps the `match strategy` block in an `if maybe_state_root.is_none()` guard.

**Strategy forced to Synchronous (line ~516):**
When `trust_consensus` is true, forces `StateRootStrategy::Synchronous` to avoid spawning expensive trie computation tasks.

#### 8. `crates/node/builder/src/launch/common.rs`
- Skip static file consistency check when `trust_consensus` is true
- Prevents startup failure when static files are corrupted

#### 9. `crates/node/core/src/node_config.rs`
- Walk-back logic: when header for the Finish checkpoint isn't found (static file corruption), walks back in 1000-block steps to find a valid header

#### 10. `crates/storage/provider/src/providers/blockchain_provider.rs`
- Walk-back logic: when BlockchainProvider initialization can't find the latest header, walks back to find a valid block

## Key Learnings

### 1. Telos Testnet Has Zero EVM Transactions
All 418M+ testnet blocks have 0 EVM transactions. State comes entirely from native-layer state diffs, not EVM execution. This means `build_state` with transaction execution enabled would do nothing — no transactions to execute.

### 2. State Diffs vs Transaction Execution
Cannot execute transactions without pre-state. The `build_state` mode must NOT execute transactions because:
- Account nonces in the DB are 0 (no historical state)
- Transaction nonce validation fails with "nonce N too high, expected 0"
- The correct approach: skip execution, apply state diffs from the CL

### 3. `compare_state_diffs()` Was Never Wired In
The function existed in `compare.rs` with full implementation (balance, nonce, code, storage overrides via `StateOverride` + `db.commit()`), but no code path ever called it. The CL writes extra fields to both filesystem and JSON-RPC params, but reth ignores both.

### 4. Changeset Cache Walking is a Critical Bottleneck
When `OverlayStateProviderFactory` is created with a `block_hash`, it tries to walk backwards through ancestor blocks via the changeset cache. With 418M+ blocks and no cache entries, this walks backwards at ~30ms per block — would take days. The fix: create the factory without `block_hash` when `trust_consensus` is true.

### 5. State Root Computation Must Be Fully Skipped
Even with `StateRootStrategy::Synchronous` forced, the parallel state root task from `spawn_payload_processor` still runs. The `maybe_state_root` pre-setting trick prevents the expensive serial fallback from running.

### 6. `kill -9` During Static File Compaction Corrupts the Database
Reth's static file compaction splits files into ranges. If the process is killed during a split, the MDBX metadata references ranges that don't exist on disk. This creates a cascading failure:
- Consistency check fails (missing files)
- Unwind fails (can't read headers from missing files)
- BlockchainProvider initialization fails (can't find the latest header)
- Node config initialization fails (same issue)

The fix required multiple bypass patches, but the underlying issue is that static file references in MDBX can't be repaired without either:
- A full re-sync
- Direct MDBX database surgery (removing stale range references)

**Recommendation:** Always use `kill` (SIGTERM) instead of `kill -9` for reth. If `kill -9` is necessary, take a backup first.

### 7. Telos CL Genesis Hash Mismatch
The CL computes EVM block hashes from native block data. The genesis hash computed by the CL doesn't match reth's chain genesis hash. This means a fresh datadir can't sync from block 1 — the CL needs to start from a block that already has its parent in reth's DB.

### 8. Gas Price on Telos Testnet
Telos testnet has extremely high gas prices (~6152 Gwei) at times, making even simple transfers cost ~0.129 TLOS. Test accounts need at least 1 TLOS for meaningful testing. Gas price 0 is NOT accepted by the native layer despite historical Telos allowing zero-gas transactions.

## Test Results

### RPC Compatibility (10/10 PASS)
All core RPC methods work correctly: `eth_blockNumber`, `eth_chainId`, `net_version`, `eth_gasPrice`, `eth_syncing`, `web3_clientVersion`, `rpc_modules`, `eth_accounts`, `eth_getBlockByNumber`.

### Block Hash Integrity (PASS)
Random block hashes verified against production — all match exactly.

### Block Structure (23/23 PASS)
All required fields present. All field values match production.

### Performance (2/2 PASS)
- `eth_blockNumber`: 6ms avg latency
- `eth_getBlockByNumber`: 6ms avg latency

### State Queries (Valid but Empty)
RPC calls return valid responses (no errors), but state values are 0 for accounts not yet touched by build_state. This is expected — historical state requires either a full re-sync with state diffs or a bulk state import.

### Live Transaction Tests (BLOCKED)
Blocked by two issues:
1. Static file corruption prevented sustained sync
2. CL instability ("Invalid forkchoice state", "Executor hash mismatch" crashes)

## Running the Node

```bash
# Pure trust_consensus (no state, fast sync)
telos-reth node --chain telos-testnet \
  --telos.trust_consensus true

# trust_consensus + build_state (state from native diffs)
telos-reth node --chain telos-testnet \
  --telos.trust_consensus true \
  --telos.build_state
```

## TODO / Next Steps

1. **Stable CL:** Debug and fix CL crashes ("Invalid forkchoice state", "Executor hash mismatch") so it stays running continuously
2. **Historical State Backfill:** Write a script to query all account states from production RPC and create synthetic state diff entries, then process them through the build_state pipeline
3. **Static File Recovery:** Implement a `telos-reth db repair-static-files` command that can detect and fix stale range references in MDBX
4. **Live TX Testing:** Once stable sync + state diffs are flowing, run full transaction tests (simple transfers, contract interactions, WTLOS deposit/withdraw)
5. **Receipt Generation:** The build_state path currently produces `BlockExecutionResult::default()` which has empty receipts. Need to either reconstruct receipts from CL extra fields or store them from the native layer
