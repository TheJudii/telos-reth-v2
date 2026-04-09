#!/usr/bin/env bash
# /usr/local/bin/telos-reth-v2-hashcheck
#
# Spot-check block hashes between the local v2 node and the public testnet
# RPC. Checks: hash, stateRoot, transactionsRoot, receiptsRoot at each
# checkpoint block.
#
# Usage: telos-reth-v2-hashcheck [extra-checkpoint ...]
# Env:
#   LOCAL   default http://127.0.0.1:8577
#   PUBLIC  default https://rpc.testnet.telos.net

set -uo pipefail
LOCAL="${LOCAL:-http://127.0.0.1:8577}"
PUBLIC="${PUBLIC:-https://rpc.testnet.telos.net}"

CHECKPOINTS=(0 1 1000 10000 100000 1000000 5000000 10000000 50000000 100000000 200000000 300000000 400000000 "$@")

command -v jq >/dev/null || { apt-get update -qq && apt-get install -y -qq jq; }

call() {
  local url=$1 payload=$2
  curl -sS --max-time 20 -X POST -H 'content-type: application/json' --data "$payload" "$url"
}

blk_by_num() {
  local url=$1 hex=$2
  call "$url" "{\"jsonrpc\":\"2.0\",\"method\":\"eth_getBlockByNumber\",\"params\":[\"${hex}\",false],\"id\":1}"
}

tip_hex() {
  call "$1" '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}' | jq -r .result
}

tip_local_hex=$(tip_hex "$LOCAL")  || tip_local_hex=0x0
tip_public_hex=$(tip_hex "$PUBLIC") || tip_public_hex=0x0
tip_local=$((tip_local_hex))
tip_public=$((tip_public_hex))

printf 'Local tip : %d (%s)\n' "$tip_local"  "$tip_local_hex"
printf 'Public tip: %d (%s)\n' "$tip_public" "$tip_public_hex"

# Always also compare the local tip block itself.
CHECKPOINTS+=("$tip_local")

# Deduplicate + keep only those <= local tip.
mapfile -t CHECKPOINTS < <(printf '%s\n' "${CHECKPOINTS[@]}" | awk -v t="$tip_local" '$0 != "" && $0+0 <= t' | sort -un)

printf '\n%-12s %-8s %-70s\n' block status details
printf -- '----------------------------------------------------------------------------------------------------\n'

fails=0
for N in "${CHECKPOINTS[@]}"; do
  HEX=$(printf "0x%x" "$N")
  LOC=$(blk_by_num "$LOCAL"  "$HEX")
  PUB=$(blk_by_num "$PUBLIC" "$HEX")

  lH=$(jq -r '.result.hash // "null"'             <<< "$LOC")
  lSR=$(jq -r '.result.stateRoot // "null"'        <<< "$LOC")
  lTR=$(jq -r '.result.transactionsRoot // "null"' <<< "$LOC")
  lRR=$(jq -r '.result.receiptsRoot // "null"'     <<< "$LOC")

  pH=$(jq -r '.result.hash // "null"'             <<< "$PUB")
  pSR=$(jq -r '.result.stateRoot // "null"'        <<< "$PUB")
  pTR=$(jq -r '.result.transactionsRoot // "null"' <<< "$PUB")
  pRR=$(jq -r '.result.receiptsRoot // "null"'     <<< "$PUB")

  if [ "$lH" = "null" ] || [ "$pH" = "null" ]; then
    printf '%-12s %-8s %s\n' "$N" "NODATA" "local=$lH public=$pH"
    fails=$((fails+1))
    continue
  fi

  if [ "$lH" = "$pH" ] && [ "$lSR" = "$pSR" ] && [ "$lTR" = "$pTR" ] && [ "$lRR" = "$pRR" ]; then
    printf '%-12s %-8s %s\n' "$N" "OK" "$lH"
  else
    printf '%-12s %-8s MISMATCH\n' "$N" "FAIL"
    printf '  hash:         local=%s\n                public=%s\n' "$lH"  "$pH"
    printf '  stateRoot:    local=%s\n                public=%s\n' "$lSR" "$pSR"
    printf '  txRoot:       local=%s\n                public=%s\n' "$lTR" "$pTR"
    printf '  receiptsRoot: local=%s\n                public=%s\n' "$lRR" "$pRR"
    fails=$((fails+1))
  fi
done

echo
if [ "$fails" -eq 0 ]; then
  echo "ALL CHECKPOINTS OK ($(printf '%s ' "${CHECKPOINTS[@]}"))"
  exit 0
else
  echo "$fails CHECKPOINT(S) FAILED"
  exit 1
fi
