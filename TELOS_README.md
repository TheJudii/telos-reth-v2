# Telos Reth v2

A fork of [reth](https://github.com/paradigmxyz/reth) v2.0.0 adapted for the Telos EVM network.

## Overview

Telos EVM is an Ethereum-compatible virtual machine running on the Telos blockchain. This fork extends the standard reth Ethereum execution client with Telos-specific functionality:

- **Transaction Forwarding**: Raw EVM transactions are forwarded to the Telos native network via the `eosio.evm` contract
- **State Diff Comparison**: Compares execution results between revm and the Telos EVM contract
- **Custom Gas Price**: Telos uses its own gas price mechanism controlled by on-chain tables
- **Block Extensions**: Additional per-block metadata (gas price changes, revision numbers)

## Telos Crates

| Crate | Description |
|-------|-------------|
| `telos-reth` | Binary - the Telos reth node executable |
| `reth-node-telos` | Node configuration and builder types |
| `reth-telos-primitives-traits` | Telos-specific primitive types (block extensions, gas prices) |
| `reth-telos-rpc` | TelosClient for native chain interaction |
| `reth-telos-rpc-engine-api` | Engine API extensions (state diffs, extra fields) |

## Building

```bash
cargo build --release --bin telos-reth
```

## Running

```bash
# Basic node
./target/release/telos-reth node --chain <chainspec.json>

# With Telos native forwarding
./target/release/telos-reth node \
  --chain <chainspec.json> \
  --telos.telos_endpoint http://localhost:8888 \
  --telos.signer_account rpc.evm \
  --telos.signer_permission active \
  --telos.signer_key <private_key>
```

## Rebase Notes

To rebase against upstream reth:

```bash
git checkout main
git fetch upstream
git rebase upstream/v2.x.x
git checkout telos-main
git rebase main
```

## Database Maintenance

To clear the freelist: `mdbx_copy -c $OLD $NEW`

See: [Reth Database Compaction](https://paradigmxyz.github.io/reth/run/troubleshooting.html#database-compaction)
