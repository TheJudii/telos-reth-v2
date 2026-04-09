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
#  - Signer credentials are reused from v1 because reth requires them
#    whenever --telos.telos_endpoint is set, and the signer path is only
#    exercised by eth_sendRawTransaction, which cannot reach this node.
#    TODO(production): rotate to a dedicated v2 signer before cut-over.
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

# Signer credentials - reused from v1 during validation.
# TODO(production): rotate to a dedicated v2 signer before cut-over.
SIGNER_ACCOUNT="rpc.evm"
SIGNER_PERMISSION="rpc"
SIGNER_KEY="5HwmX44dc1optAssMvdAJZe2qvHwbkZogiu4uij2aDPmZLEcN2s"

mkdir -p "${DATADIR}"

# The JWT MUST match the value in the telos-consensus-client config
# (testnet-genesis/config.toml `jwt_secret`). If the file is missing,
# seed it with the shared secret so both sides agree on day one.
SHARED_JWT="1dec866571dc8725b048e672bfd1dc5c0c3afe5e3180f707776ec908188b9084"
if [ ! -f "${JWT}" ]; then
  printf '%s' "${SHARED_JWT}" > "${JWT}"
  chmod 600 "${JWT}"
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
  --engine.persistence-threshold 2 \
  --engine.persistence-backpressure-threshold 16 \
  --telos.telos_endpoint "${TELOS_ENDPOINT}" \
  --telos.signer_account "${SIGNER_ACCOUNT}" \
  --telos.signer_permission "${SIGNER_PERMISSION}" \
  --telos.signer_key "${SIGNER_KEY}"
