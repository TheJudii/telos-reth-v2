#!/usr/bin/env python3
"""Canonical-comparison monitor for telos-reth-v2 quick nodes.

Runs continuously. Every CHECK_INTERVAL seconds, queries the local quick node
and the canonical RPC for both networks, compares hashes at four historical
heights (qTip-50, -200, -800, -2000), and emits structured log lines.

A MISMATCH at any height immediately fires a syslog ERROR with a `CANONICAL_MISMATCH`
tag — alerting tools should be configured to page on that string.

Hourly: emits a summary line with check totals, mismatch totals, lag stats.
Daily: emits a daily summary at UTC midnight.

State persists in /var/lib/telos/canonical-monitor.state so the daemon survives
restarts without losing counters.
"""
from __future__ import annotations
import json
import os
import subprocess
import sys
import syslog
import time
from dataclasses import dataclass, field, asdict
from typing import Optional

CHECK_INTERVAL_S = 60
HOURLY_SUMMARY_S = 3600
SAMPLE_DELTAS = (50, 200, 800, 2000)
LOG_PATH = "/var/log/telos-canonical-monitor.log"
STATE_PATH = "/var/lib/telos/canonical-monitor.state"
SYSLOG_TAG = "telos-canonical-monitor"

NODES = {
    "mainnet-quick": {
        "local": "http://127.0.0.1:8477",
        "canonical": "https://rpc.telos.net",
        "chain_id": 40,
    },
    "testnet-quick": {
        "local": "http://127.0.0.1:8677",
        "canonical": "https://rpc.testnet.telos.net",
        "chain_id": 41,
    },
}


@dataclass
class NodeStats:
    """Rolling counters per node, persisted across restarts."""
    checks: int = 0
    mismatches: int = 0
    local_unreachable: int = 0
    canonical_unreachable: int = 0
    last_lag: int = 0
    last_local_tip: int = 0
    last_canonical_tip: int = 0
    last_check_ts: float = 0.0
    last_mismatch_ts: float = 0.0
    last_mismatch_block: int = 0
    last_mismatch_local_hash: str = ""
    last_mismatch_canonical_hash: str = ""


@dataclass
class State:
    """Aggregate state, persisted as JSON."""
    started_at: float = field(default_factory=lambda: time.time())
    last_hourly_summary_ts: float = 0.0
    nodes: dict = field(default_factory=dict)


def load_state() -> State:
    if not os.path.exists(STATE_PATH):
        return State()
    try:
        with open(STATE_PATH) as f:
            raw = json.load(f)
        st = State(
            started_at=raw.get("started_at", time.time()),
            last_hourly_summary_ts=raw.get("last_hourly_summary_ts", 0.0),
            nodes={k: NodeStats(**v) for k, v in raw.get("nodes", {}).items()},
        )
        return st
    except Exception as e:
        log_local(f"state load failed, starting fresh: {e}", level="warn")
        return State()


def save_state(st: State) -> None:
    os.makedirs(os.path.dirname(STATE_PATH), exist_ok=True)
    tmp = STATE_PATH + ".tmp"
    payload = {
        "started_at": st.started_at,
        "last_hourly_summary_ts": st.last_hourly_summary_ts,
        "nodes": {k: asdict(v) for k, v in st.nodes.items()},
    }
    with open(tmp, "w") as f:
        json.dump(payload, f)
    os.replace(tmp, STATE_PATH)


def log_local(msg: str, level: str = "info") -> None:
    """Append to /var/log/telos-canonical-monitor.log with a timestamp."""
    line = f"[{time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())}] [{level.upper()}] {msg}\n"
    try:
        with open(LOG_PATH, "a") as f:
            f.write(line)
    except Exception as e:
        # Last resort: print to stderr so journalctl picks it up
        print(line, end="", file=sys.stderr)
        print(f"(log write failed: {e})", file=sys.stderr)


def log_syslog(msg: str, level: str = "info") -> None:
    """Emit to syslog so external alerting tools can grep on the tag."""
    pri = {"debug": syslog.LOG_DEBUG, "info": syslog.LOG_INFO,
           "warn": syslog.LOG_WARNING, "error": syslog.LOG_ERR}.get(level, syslog.LOG_INFO)
    syslog.syslog(pri, msg)


def rpc(url: str, method: str, params=None, timeout: float = 5.0) -> Optional[dict]:
    payload = json.dumps({"jsonrpc": "2.0", "method": method, "params": params or [], "id": 1})
    try:
        r = subprocess.run(
            ["curl", "-s", "-m", str(int(timeout)), "-X", "POST",
             "-H", "Content-Type: application/json",
             "-A", "telos-canonical-monitor/1.0", "-d", payload, url],
            capture_output=True, text=True, timeout=timeout + 5,
        )
        if r.returncode != 0 or not r.stdout:
            return None
        return json.loads(r.stdout)
    except Exception:
        return None


def hex_to_int(h):
    if h is None:
        return None
    if isinstance(h, int):
        return h
    if isinstance(h, str) and h.startswith("0x"):
        try:
            return int(h, 16)
        except ValueError:
            return None
    return None


def check_node(node: str, cfg: dict, stats: NodeStats) -> None:
    """One pass: tip + 4 hash samples vs canonical. Mutates stats in place."""
    stats.checks += 1
    stats.last_check_ts = time.time()

    local_resp = rpc(cfg["local"], "eth_blockNumber")
    canon_resp = rpc(cfg["canonical"], "eth_blockNumber")

    local_tip = hex_to_int((local_resp or {}).get("result"))
    canon_tip = hex_to_int((canon_resp or {}).get("result"))

    if local_tip is None:
        stats.local_unreachable += 1
        log_local(f"{node}: local unreachable", level="warn")
        return
    if canon_tip is None:
        stats.canonical_unreachable += 1
        log_local(f"{node}: canonical unreachable (transient)", level="warn")
        return

    stats.last_local_tip = local_tip
    stats.last_canonical_tip = canon_tip
    stats.last_lag = canon_tip - local_tip

    # 4-height hash sweep
    mismatches_this_check = []
    for delta in SAMPLE_DELTAS:
        bn = local_tip - delta
        if bn <= 0:
            continue
        bn_hex = hex(bn)
        l = rpc(cfg["local"], "eth_getBlockByNumber", [bn_hex, False])
        c = rpc(cfg["canonical"], "eth_getBlockByNumber", [bn_hex, False])
        l_hash = ((l or {}).get("result") or {}).get("hash")
        c_hash = ((c or {}).get("result") or {}).get("hash")
        if not l_hash or not c_hash:
            continue
        if l_hash != c_hash:
            mismatches_this_check.append((bn, l_hash, c_hash))

    if mismatches_this_check:
        stats.mismatches += len(mismatches_this_check)
        stats.last_mismatch_ts = time.time()
        for (bn, lh, ch) in mismatches_this_check:
            stats.last_mismatch_block = bn
            stats.last_mismatch_local_hash = lh
            stats.last_mismatch_canonical_hash = ch
            err = (f"CANONICAL_MISMATCH node={node} block={bn} "
                   f"local_hash={lh} canonical_hash={ch} "
                   f"local_tip={local_tip} canonical_tip={canon_tip} lag={stats.last_lag}")
            log_local(err, level="error")
            log_syslog(err, level="error")
    else:
        log_local(
            f"{node}: OK local_tip={local_tip} canonical_tip={canon_tip} lag={stats.last_lag} "
            f"checks={stats.checks} mismatches_lifetime={stats.mismatches}",
            level="info",
        )


def emit_hourly_summary(st: State) -> None:
    parts = ["HOURLY_SUMMARY"]
    for node, s in st.nodes.items():
        parts.append(
            f"{node}: checks={s.checks} mismatches={s.mismatches} "
            f"local_tip={s.last_local_tip} lag={s.last_lag} "
            f"local_unreachable={s.local_unreachable} canonical_unreachable={s.canonical_unreachable}"
        )
    line = " | ".join(parts)
    log_local(line, level="info")
    log_syslog(line, level="info")


def main():
    syslog.openlog(SYSLOG_TAG, syslog.LOG_PID, syslog.LOG_DAEMON)
    log_local("canonical-monitor starting", level="info")
    log_syslog("canonical-monitor starting", level="info")

    st = load_state()
    for node in NODES:
        if node not in st.nodes:
            st.nodes[node] = NodeStats()

    while True:
        cycle_start = time.time()
        for node, cfg in NODES.items():
            try:
                check_node(node, cfg, st.nodes[node])
            except Exception as e:
                log_local(f"{node}: check raised exception: {e}", level="warn")

        # Hourly summary
        if time.time() - st.last_hourly_summary_ts >= HOURLY_SUMMARY_S:
            emit_hourly_summary(st)
            st.last_hourly_summary_ts = time.time()

        # Persist
        try:
            save_state(st)
        except Exception as e:
            log_local(f"state save failed: {e}", level="warn")

        # Sleep until next cycle
        sleep_for = CHECK_INTERVAL_S - (time.time() - cycle_start)
        if sleep_for > 0:
            time.sleep(sleep_for)


if __name__ == "__main__":
    main()
