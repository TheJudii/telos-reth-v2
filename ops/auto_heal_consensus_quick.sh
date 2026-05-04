#!/usr/bin/env bash
# Auto-heal daemon for telos-consensus-quick.
#
# Watches the telos-consensus-quick journal for the known bad-state pattern:
#   "Executor hash mismatch"
# and performs the runbook recovery:
#   systemctl restart telos-reth-quick
#   sleep 10
#   systemctl restart telos-consensus-quick
#
# Invariants:
#   - Maximum one auto-heal per 30 minutes (cooldown) to avoid oscillation.
#   - Heal attempts counted in /var/lib/telos/autoheal.counter; after 3 attempts
#     in a 2-hour window, daemon stops itself and pages (exit non-zero — the
#     systemd OnFailure= hook sends the page).
#   - Log actions to /var/log/telos-autoheal.log AND systemd journal.
#
# Run as systemd service: autoheal-consensus-quick.service (included).

set -euo pipefail

LOG=/var/log/telos-autoheal.log
STATE_DIR=/var/lib/telos
COUNTER=$STATE_DIR/autoheal.counter
LAST_HEAL=$STATE_DIR/autoheal.last

COOLDOWN_S=1800           # 30 min between heals
WINDOW_S=7200             # 2-hour rolling window
MAX_IN_WINDOW=3

mkdir -p "$STATE_DIR"
: > "$LOG" 2>/dev/null || true

log() {
    local ts
    ts=$(date -u +%Y-%m-%dT%H:%M:%SZ)
    echo "[$ts] $*" | tee -a "$LOG"
    logger -t telos-autoheal "$*"
}

record_heal() {
    local now
    now=$(date +%s)
    echo "$now" >> "$COUNTER"
    echo "$now" > "$LAST_HEAL"
    # keep only entries within window
    local cutoff=$(( now - WINDOW_S ))
    awk -v c="$cutoff" '$1 >= c' "$COUNTER" > "$COUNTER.tmp" && mv "$COUNTER.tmp" "$COUNTER"
}

heals_in_window() {
    if [[ ! -f "$COUNTER" ]]; then echo 0; return; fi
    local now cutoff
    now=$(date +%s)
    cutoff=$(( now - WINDOW_S ))
    awk -v c="$cutoff" '$1 >= c' "$COUNTER" | wc -l
}

time_since_last_heal() {
    if [[ ! -f "$LAST_HEAL" ]]; then echo 99999999; return; fi
    local last now
    last=$(cat "$LAST_HEAL")
    now=$(date +%s)
    echo $(( now - last ))
}

attempt_heal() {
    local reason=$1
    local since
    since=$(time_since_last_heal)
    if (( since < COOLDOWN_S )); then
        log "SUPPRESS: $reason detected but last heal was ${since}s ago (cooldown ${COOLDOWN_S}s)"
        return 0
    fi
    local count
    count=$(heals_in_window)
    if (( count >= MAX_IN_WINDOW )); then
        log "ESCALATE: $count heals in the last ${WINDOW_S}s — giving up, please investigate"
        exit 3    # OnFailure page
    fi

    log "HEAL: $reason — running restart sequence (heal #$((count + 1)) in window)"
    systemctl restart telos-reth-quick || log "WARN: reth-quick restart returned non-zero"
    sleep 10
    systemctl restart telos-consensus-quick || log "WARN: consensus-quick restart returned non-zero"
    record_heal
    log "HEAL: complete"
}

log "autoheal daemon starting (PID $$)"
# Tail journal, check for known patterns
journalctl -u telos-consensus-quick -f --since now --no-pager 2>&1 | \
while IFS= read -r line; do
    if echo "$line" | grep -q "Executor hash mismatch"; then
        attempt_heal "Executor hash mismatch"
    fi
    # Future patterns get added here — one elif per known bad state.
done

log "autoheal daemon exiting (journalctl pipe closed)"
exit 1
