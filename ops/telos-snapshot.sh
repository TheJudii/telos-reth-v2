#!/usr/bin/env bash
#
# Daily snapshot of reth + CL DB for both mainnet-quick and testnet-quick.
# Cold-copy: stops services, cp -a, restarts services. ~30-60s downtime per node.
#
# Snapshots live under /data/backups/auto/YYYY-MM-DD/<node>/{reth-datadir,cl-db}/
# Retention: 7 daily snapshots. Older snapshots are deleted at the end of each run.
#
# Safety:
#  - Refuses to run if services have been up for less than 12 hours (avoids
#    snapshotting state that might still be settling after a deploy).
#  - Refuses to run if /data has less than 5 GB free.
#  - Each node is snapshotted independently; failure of one doesn't abort the other.
#
set -uo pipefail

SNAP_ROOT=/data/backups/auto
RETENTION_DAYS=7
MIN_UPTIME_S=43200       # 12 h
MIN_FREE_GB=5
LOG=/var/log/telos-snapshot.log

DATE_TAG=$(date -u +%Y-%m-%d)
DATE_DIR=$SNAP_ROOT/$DATE_TAG

log() {
    echo "[$(date -u +%Y-%m-%dT%H:%M:%SZ)] $*" | tee -a "$LOG"
}

snapshot_node() {
    local node=$1
    local reth_datadir=$2
    local cl_dbpath=$3
    local cl_unit="telos-consensus-${node}"
    local reth_unit="telos-reth-${node}"

    log "==> snapshot $node start"

    # Uptime check
    local active_ts
    active_ts=$(systemctl show -p ActiveEnterTimestamp --value "$reth_unit")
    if [ -z "$active_ts" ]; then
        log "    WARN: $reth_unit has no ActiveEnterTimestamp; skipping"
        return 1
    fi
    local active_epoch
    active_epoch=$(date -d "$active_ts" +%s 2>/dev/null || echo 0)
    local now=$(date -u +%s)
    local uptime=$((now - active_epoch))
    if [ "$uptime" -lt "$MIN_UPTIME_S" ]; then
        log "    SKIP: $node up only ${uptime}s (< ${MIN_UPTIME_S}s minimum)"
        return 0
    fi
    log "    $node uptime ${uptime}s OK"

    # Free space check
    local free_kb
    free_kb=$(df --output=avail -k /data | tail -1)
    local free_gb=$((free_kb / 1024 / 1024))
    if [ "$free_gb" -lt "$MIN_FREE_GB" ]; then
        log "    ABORT: only ${free_gb} GB free on /data (< ${MIN_FREE_GB} GB)"
        return 1
    fi

    local target=$DATE_DIR/$node
    mkdir -p "$target"

    # Stop in order: CL first, then reth.
    log "    stopping services"
    systemctl stop "$cl_unit"
    systemctl stop "$reth_unit"

    # Cold-copy. Using cp -a so perms + symlinks preserved.
    local copy_start=$(date -u +%s)
    log "    copying $reth_datadir -> $target/reth-datadir"
    if ! cp -a "$reth_datadir" "$target/reth-datadir"; then
        log "    ERROR: reth datadir copy failed"
        # Still start services back up before bailing
        systemctl start "$reth_unit"
        sleep 8
        systemctl start "$cl_unit"
        return 1
    fi
    log "    copying $cl_dbpath -> $target/cl-db"
    if ! cp -a "$cl_dbpath" "$target/cl-db"; then
        log "    ERROR: CL DB copy failed"
        systemctl start "$reth_unit"
        sleep 8
        systemctl start "$cl_unit"
        return 1
    fi
    local copy_elapsed=$(( $(date -u +%s) - copy_start ))

    # Backup the service unit + launcher (they're tiny but matter for full rollback).
    cp "/etc/systemd/system/${cl_unit}.service"  "$target/" 2>/dev/null || true
    cp "/usr/local/bin/telos-reth-v2-${node}"     "$target/" 2>/dev/null || true

    # Manifest
    {
        echo "snapshot tag: $DATE_TAG"
        echo "node: $node"
        echo "taken at (UTC): $(date -u +%Y-%m-%dT%H:%M:%SZ)"
        echo "reth datadir size: $(du -sh "$target/reth-datadir" | cut -f1)"
        echo "cl db size: $(du -sh "$target/cl-db" | cut -f1)"
        echo "copy elapsed: ${copy_elapsed}s"
        echo "uptime at snapshot: ${uptime}s"
    } > "$target/manifest.txt"

    # Restart services
    log "    restarting services"
    systemctl start "$reth_unit"
    sleep 8
    systemctl start "$cl_unit"

    # Verify both came back up
    sleep 3
    if [ "$(systemctl is-active "$reth_unit")" != "active" ] || \
       [ "$(systemctl is-active "$cl_unit")" != "active" ]; then
        log "    ERROR: $node services failed to come back active after snapshot"
        return 1
    fi

    log "==> snapshot $node OK (copy ${copy_elapsed}s)"
    return 0
}

prune_old() {
    log "==> pruning snapshots older than ${RETENTION_DAYS} days"
    if [ ! -d "$SNAP_ROOT" ]; then return 0; fi
    find "$SNAP_ROOT" -mindepth 1 -maxdepth 1 -type d -mtime "+$RETENTION_DAYS" -print -exec rm -rf {} +
}

main() {
    log "###### snapshot run start tag=$DATE_TAG ######"

    # mainnet-quick
    snapshot_node mainnet-quick \
        /data/reth-mainnet-v2-quick \
        /data/telos-consensus-client/mainnet-v2-quick/db

    # testnet-quick
    snapshot_node testnet-quick \
        /data/reth-testnet-v2-quick \
        /data/telos-consensus-client/testnet-v2-quick/db

    prune_old

    # Final summary
    log "==> retained snapshots:"
    ls -la "$SNAP_ROOT" 2>/dev/null | tail -n +2 | tee -a "$LOG"
    log "==> total backups size: $(du -sh "$SNAP_ROOT" 2>/dev/null | cut -f1)"

    log "###### snapshot run done ######"
}

main
