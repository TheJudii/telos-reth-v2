#!/usr/bin/env bash
# /usr/local/bin/telos-reth-v2
#
# Launcher for the side-by-side telos-reth v2 validation node.
#
# Design notes:
#  - Runs alongside the existing v1.11.3 node on a separate datadir and
#    port set. HTTP/WS RPC and auth RPC are bound to 127.0.0.1 only.
#  - This node is NOT intended to serve external traffic during
#    validation; it exists to independently sync testnet from genesis
#    so we can diff block hashes against the live network.
#  - Blocks are driven into this node over the Engine API by a dedicated
#    telos-consensus-client instance (see
#    /data/telos-consensus-client/testnet-genesis/config.toml), which
#    reads from nodeos state-history and pushes engine_newPayload +
#    engine_forkchoiceUpdated into authrpc :8579.
#  - Pre-Savannah head-tracking keeps a 20-block persistence window so
#    reversible-fork state remains in memory instead of being flushed to MDBX.
#  - Signer credentials are reused from v1 because reth requires them
#    whenever --telos.telos_endpoint is set, and the signer path is
#    only exercised by eth_sendRawTransaction, which cannot reach
#    this node.
#
# Signer WIF:
#  - The rpc.evm@rpc forwarder key is a shared operator credential
#    intended to be publicly distributed with Telos RPC node
#    software. Its on-chain permission is linked only to
#    eosio.evm::raw, ::call, and ::delreciepts, so it has no
#    authority to move funds, touch mainnet, or modify its own keys.
#    We commit the canonical public default here so the node works
#    out of the box, but it can be overridden per-deployment via
#    the SIGNER_KEY env var or /etc/telos/signer.key (mode 0600).
#
# Engine API JWT:
#  - The JWT is read from ${JWT} (default /data/reth-testnet-v2/jwt.hex).
#    Unlike the signer WIF, the JWT is a per-deployment symmetric
#    secret shared only between this reth and its consensus-client;
#    it is deliberately NOT committed. The operator seeds it once
#    via e.g. 'openssl rand -hex 32 > /data/reth-testnet-v2/jwt.hex'
#    and copies the same hex string into the consensus client's
#    jwt_secret config field.
#
# Port layout (v1 live / v2 side-by-side):
#   HTTP RPC     8557 / 8577
#   WS RPC       8558 / 8578
#   Auth RPC     8559 / 8579  (localhost only on both)
#   p2p/disc    30333 / 30335
#   Metrics     (v1 unset) / 9002 (localhost only)

set -euo pipefail

BIN=/data/telos-reth-v2/target/release/telos-reth
DATADIR=/data/reth-testnet-v2
CONFIG="${DATADIR}/reth.toml"
JWT="${DATADIR}/jwt.hex"

# Telos native (nodeos) endpoint used by reth's signer path for
# eth_sendRawTransaction forwarding. Block-sync does NOT use this.
TELOS_ENDPOINT="http://127.0.0.1:18889"

# Signer credentials. The account and permission are public
# identifiers. The default WIF below is the historical shared
# rpc.evm@rpc forwarder key; production operators should verify it
# against the current on-chain rpc.evm@rpc key or override it via
# SIGNER_KEY / /etc/telos/signer.key.
SIGNER_ACCOUNT="${SIGNER_ACCOUNT:-rpc.evm}"
SIGNER_PERMISSION="${SIGNER_PERMISSION:-rpc}"
DEFAULT_SIGNER_KEY="5HwmX44dc1optAssMvdAJZe2qvHwbkZogiu4uij2aDPmZLEcN2s"

# Resolve SIGNER_KEY. Precedence:
#   1. $SIGNER_KEY env var (useful for ad-hoc runs / alt accounts)
#   2. /etc/telos/signer.key (useful when an operator provisions a
#      custom key per deployment via config management)
#   3. The committed public default above.
if [ -z "${SIGNER_KEY:-}" ]; then
  if [ -r /etc/telos/signer.key ]; then
    SIGNER_KEY="$(head -n1 /etc/telos/signer.key | tr -d '[:space:]')"
  fi
fi
SIGNER_KEY="${SIGNER_KEY:-$DEFAULT_SIGNER_KEY}"

mkdir -p "${DATADIR}"

# The JWT MUST match the value in the telos-consensus-client config
# (testnet-genesis/config.toml `jwt_secret`). We do not seed it from a
# committed default; the operator is expected to create jwt.hex once
# via `openssl rand -hex 32 > ${JWT}` and mirror the same hex into the
# CL config.
if [ ! -f "${JWT}" ]; then
  echo "telos-reth-v2: JWT file ${JWT} is missing." >&2
  echo "telos-reth-v2: create it with 'openssl rand -hex 32 > ${JWT} && chmod 600 ${JWT}'" >&2
  echo "telos-reth-v2: and copy the same hex string into the consensus client's jwt_secret." >&2
  exit 2
fi

[ -f "${CONFIG}" ] || touch "${CONFIG}"

exec "${BIN}" node \
  --chain telos-testnet \
  --datadir "${DATADIR}" \
  --config "${CONFIG}" \
  --http --http.addr 127.0.0.1 --http.port 8577 --http.api all \
  --ws   --ws.addr   127.0.0.1 --ws.port   8578 --ws.api   all \
  --authrpc.addr 127.0.0.1 --authrpc.port 8579 \
  --authrpc.jwtsecret "${JWT}" \
  --ipcpath "${DATADIR}/reth.ipc" \
  --port 30335 --discovery.port 30335 \
  --metrics 127.0.0.1:9002 \
  --engine.persistence-threshold 20 \
  --engine.persistence-backpressure-threshold 30 \
  --telos.telos_endpoint "${TELOS_ENDPOINT}" \
  --telos.signer_account "${SIGNER_ACCOUNT}" \
  --telos.signer_permission "${SIGNER_PERMISSION}" \
  --telos.signer_key "${SIGNER_KEY}"
