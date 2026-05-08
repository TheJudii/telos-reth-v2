# telos-reth v2 side-by-side validation node

Setup notes for bringing up a second, from-genesis telos-reth v2 node
on a host that is already running the production v1.11.3 stack, so
that its block hashes can be diffed against the live network
(`https://rpc.testnet.telos.net`) before cut-over.

The target host for this write-up is the Hetzner box at
`135.181.1.160`, which already runs:

- `telos-reth` 1.11.3 on ports 8557/8558/8559, datadir
  `/data/reth-testnet-v1.11.3`.
- `telos-consensus-client` (the "v1113" instance), driving v1 reth
  over authrpc :8559.
- `nodeos` on 127.0.0.1:18889 (HTTP) and 0.0.0.0:18081 (state-history
  WebSocket, SHIP).
- `elasticsearch`, `hyperion`, `rabbitmq`, `redis`, `nginx`, etc. —
  untouched by this work.

The v2 node runs **entirely on localhost** and is not exposed through
nginx or the external LB. It is for hash-validation only.

## 1. Source, patches, and build

Upstream: `https://github.com/TheJudii/telos-reth-v2`.

This branch carries two patches that are required to sync from
genesis against the live testnet:

1. `fix(telos-node): drop duplicate engine.* flags in TelosArgs`
   (see `patches/0001-telos-args-drop-duplicate-engine-flags.patch`)

   `TelosArgs` declared its own `--engine.persistence-threshold` and
   `--engine.memory-block-buffer-target` in addition to the ones
   upstream `EngineArgs` already exposes. When two `#[arg(long = ...)]`
   fields in the same Command share a long name, clap silently drops
   one of the two `long` attributes and demotes that field to a
   required positional arg named by the Rust field. The result is
   that on every startup the binary exits 2 with:

   ```
   error: the following required argument was not provided: persistence_threshold
   Usage: Reth [OPTIONS] <COMMAND>
   ```

   Both duplicate fields were dead code (no call site ever read them),
   so the fix is to delete them and let upstream `EngineArgs` own
   those knobs. The third telos-specific knob,
   `--engine.max-execute-block-batch-size`, does not clash and is
   kept.

2. `fix(chainspec): restore canonical extraData in telos-testnet genesis`
   (see `patches/0002-telos-testnet-chainspec-extradata.patch`)

   The bundled `telos-testnet` chain spec had
   `"extraData": "0x"`, which produces a block-0 hash of
   `0x53e7ff3c9de5b45ce2efab06c942f8ce7dbcf7c4d2a080b1a3486de6ac641a56`.
   The canonical live-network genesis uses
   `0x000000397128c497668c241b27d1521c764156cea50bcac87892fc8916e23b24`,
   which produces
   `0xb25034033c9ca7a40e879ddcc29cf69071a22df06688b5fe8cc2d68b4e0528f9`
   — matching v1 and public RPC. Without this, the consensus client's
   `prev_hash` checkpoint for `evm_start_block = 1` disagrees with
   reth and no blocks can be pushed in.

Build:

```bash
cd /data/telos-reth-v2
cargo build --release -p telos-reth --bin telos-reth -j 16
```

The resulting binary is `/data/telos-reth-v2/target/release/telos-reth`.
It reports `Reth Version: 2.0.0`.

## 2. Datadir and JWT

```bash
mkdir -p /data/reth-testnet-v2
# Generate a fresh 32-byte JWT secret for the Engine API. This value
# is deliberately NOT committed anywhere in the repo. Create it once
# on the host and mirror the same hex string into the consensus
# client's jwt_secret field (see section 4).
openssl rand -hex 32 > /data/reth-testnet-v2/jwt.hex
chmod 600 /data/reth-testnet-v2/jwt.hex
```

The launcher in `/usr/local/bin/telos-reth-v2` expects `jwt.hex` to
already exist at startup and will refuse to boot if it does not.

## 2b. Signer WIF

The v2 reth binary requires a signer WIF on the command line whenever
`--telos.telos_endpoint` is set (the forwarder path for
`eth_sendRawTransaction`). The Telos `rpc.evm@rpc` forwarder key is
a shared operator credential intended to be publicly distributed
with Telos RPC node software: its on-chain permission is linked
only to `eosio.evm::raw`, `::call`, and `::delreciepts`, so it has
no authority to move funds, touch mainnet, or modify its own keys.
The launcher ships the canonical public default baked in
(corresponds to `EOS5D53o69eaiH7GhhCiL9Hny43iNNa8hzF2ekS7hSmFMWYoBKLy6`),
so the node works out of the box with no extra provisioning.

To override with a different WIF (e.g. a private devnet signer, or
a per-operator key), use either of:

```bash
# Option A: file-on-disk (preferred for long-running services)
install -m 0700 -d /etc/telos
printf '%s\n' 'PASTE_WIF_HERE' > /etc/telos/signer.key
chmod 600 /etc/telos/signer.key

# Option B: env var (useful for ad-hoc runs)
SIGNER_KEY='PASTE_WIF_HERE' /usr/local/bin/telos-reth-v2
```

The launcher resolves `SIGNER_KEY` in the order: `$SIGNER_KEY` env
var, then `/etc/telos/signer.key`, then the committed public
default.

## 2c. Forwarder retry window

`eth_sendRawTransaction` forwards through nodeos `send_transaction2` with
`retry_trx = true`. Public RPC nodes should not use the old two-block retry
window: a non-producing relay can validate a transaction locally and still miss
producer propagation before two 0.5s blocks pass. The default is now 120 native
blocks, and operators can override it with:

```bash
TELOS_TX_RETRY_BLOCKS=120 /usr/local/bin/telos-reth-v2
```

## 3. Launcher, systemd unit, hash-check script

The canonical copies of these three files live inside this repo under
`scripts/v2-validation/`. Install them on the host as follows:

```bash
install -m 0755 scripts/v2-validation/telos-reth-v2-launcher.sh \
  /usr/local/bin/telos-reth-v2
install -m 0644 scripts/v2-validation/telos-reth-v2.service \
  /etc/systemd/system/telos-reth-v2.service
install -m 0755 scripts/v2-validation/telos-reth-v2-hashcheck.sh \
  /usr/local/bin/telos-reth-v2-hashcheck

systemctl daemon-reload
systemctl enable telos-reth-v2
```

The launcher binds:

| role      | v1 (live) | v2 (validation) |
|-----------|-----------|-----------------|
| HTTP RPC  | 8557      | 8577            |
| WS  RPC   | 8558      | 8578            |
| auth RPC  | 8559      | 8579            |
| p2p/disc  | 30333     | 30335           |
| metrics   | (none)    | 9002 (lo)       |

All v2 ports except p2p are bound to 127.0.0.1 only.

## 4. Consensus client (block driver)

The `telos-reth` execution layer does not sync on its own — blocks
come from `telos-consensus-client`, which tails the local `nodeos`
SHIP WebSocket and pushes EVM payloads into reth over the Engine API.
A v2-specific CL instance lives at
`/data/telos-consensus-client/testnet-genesis/` and is configured to
target authrpc `:8579`. Its config file is mirrored in this repo at
`scripts/v2-validation/telos-consensus-client-v2-config.toml` with
the `jwt_secret` field left as a placeholder; before starting the
service, copy the same 64-character hex string you wrote into
`/data/reth-testnet-v2/jwt.hex` into that field.

Key fields:

- `execution_endpoint = "http://localhost:8579"` — v2 reth authrpc
- `jwt_secret` — must match the bytes in
  `/data/reth-testnet-v2/jwt.hex`; filled in per-deployment, never
  committed
- `prev_hash` — the canonical testnet block 0 hash; must match what
  reth reports for block 0
- `evm_start_block = 1` — start replay at block 1 (genesis is block 0)
- `evm_deploy_block = 137430500` — antelope block where the EVM
  contract was deployed
- `ship_endpoint = "ws://localhost:18081"` — nodeos SHIP
- `chain_endpoint = "http://localhost:18889"` — nodeos HTTP

For a clean from-genesis replay, wipe the CL's own progress db
**and** reth's datadir before starting:

```bash
systemctl stop telos-reth-v2
rm -rf /data/reth-testnet-v2/{db,static_files,blobstore,rocksdb}
rm -rf /data/telos-consensus-client/testnet-genesis/db/*
```

Start them in order:

```bash
systemctl start telos-reth-v2
# verify genesis hash matches
curl -s -X POST -H 'content-type: application/json' \
  --data '{"jsonrpc":"2.0","method":"eth_getBlockByNumber","params":["0x0",false],"id":1}' \
  http://127.0.0.1:8577 | jq -r .result.hash
# expected: 0xb25034033c9ca7a40e879ddcc29cf69071a22df06688b5fe8cc2d68b4e0528f9

cd /data/telos-consensus-client
nohup ./target/release/telos-consensus-client \
  --config testnet-genesis/config.toml \
  > testnet-genesis/genesis-sync.log 2>&1 &
echo $! > testnet-genesis/consensus.pid
```

## 5. Hash spot-check

Run `/usr/local/bin/telos-reth-v2-hashcheck` to compare the v2 node's
`hash`, `stateRoot`, `transactionsRoot`, and `receiptsRoot` at fixed
checkpoints (0, 1, 1k, 10k, 100k, 1M, 5M, 10M, 50M, 100M, 200M, 300M,
400M, plus the current local tip) against the public RPC. The script
skips checkpoints ahead of the local tip, so it is safe to run while
the sync is still in progress — every checkpoint that *is* reached
must be hash-equal to public before the v2 node can be considered a
drop-in replacement.

## 6. Production hardening (not yet done)

- log rotation for the reth journald stream and the consensus-client
  log file (the v9 log on this host is already 722 MB)
- enable prometheus scraping of `127.0.0.1:9002`
- firewall: verify UFW / iptables does not expose 8577–8579 or 9002
- crash-restart test: kill -9 the reth process and confirm systemd
  restarts it inside 10 s
- rotate `telos.signer_key` to a v2-only account before promoting
  this node behind the load balancer

## 7. Reproducing the builds in a fresh tree

```bash
git clone https://github.com/TheJudii/telos-reth-v2.git
cd telos-reth-v2
git checkout claude/telos-reth-fork-jedxC   # the branch carrying the patches
cargo build --release -p telos-reth --bin telos-reth
```
