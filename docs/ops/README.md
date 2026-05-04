# `docs/ops/` and `ops/` — operational documentation and tooling

This directory contains operational documentation and runtime tooling for the telos-reth v2 quick-sync stack. It is the companion to similar content in [TheJudii/telos-consensus-client](https://github.com/TheJudii/telos-consensus-client) — the consensus-client repo carries the source-side incident artifacts (post-mortem, source-change archive branch); this repo carries the reth-specific design specs and the cross-cutting operational tooling.

For the **2026-04-30 fork-handling MVP incident** post-mortem and full timeline, see:
[telos-consensus-client `archive/2026-04-30-fork-handling-mvp-attempted-and-reverted` branch, `docs/ops/POSTMORTEM_2026-04-30_fork_mvp.md`](https://github.com/TheJudii/telos-consensus-client/blob/archive/2026-04-30-fork-handling-mvp-attempted-and-reverted/docs/ops/POSTMORTEM_2026-04-30_fork_mvp.md)

## Index

### Design specs (reth-side changes)

| File | Purpose |
| --- | --- |
| `docs/ops/SPEC_C9_build_state_inverse.md` | Either implement an inverse for `build_state` extra-field application, or switch to revm-execution mode. Recommendation: revm-execution mode (Option B) for production-grade reth nodes. |

### Investigations (reth's compare module)

| File | Purpose |
| --- | --- |
| `docs/ops/F2_INVESTIGATION_revm_tevm_compare.md` | Diagnosis of the 9k-warning/day revm/tevm compare warnings in `crates/telos/rpc-engine-api/src/compare.rs`. Concludes the warnings are structural to `build_state` mode, not bugs. Recommends formal acceptance + log-level downgrade + rate-spike monitoring. |

### Cross-cutting docs (also mirrored in telos-consensus-client repo)

| File | Purpose |
| --- | --- |
| `docs/ops/CLIENT_CONTRACT_LIB_TRACKING.md` | Client-facing contract for the v2 quick node's LIB-tracking semantics (block tags, lag, what's guaranteed). |
| `docs/ops/PRE_DEPLOY_BACKUP_RUNBOOK.md` | Mandatory pre-deploy snapshot procedure for any change that touches reth's `--datadir`, persistence flags, or build_state behavior. |

### Operational runtime tooling

The `ops/` directory contains the actual runtime artifacts deployed on production nodes:

| File | Installed at | Purpose |
| --- | --- | --- |
| `ops/canonical-monitor.py` | `/usr/local/bin/canonical-monitor.py` | Continuously compares both quick nodes against canonical RPC. Emits hourly summaries; fires `CANONICAL_MISMATCH` to syslog on any divergence. |
| `ops/telos-canonical-monitor.service` | `/etc/systemd/system/` | systemd service unit for the monitor. |
| `ops/telos-snapshot.sh` | `/usr/local/bin/telos-snapshot.sh` | Daily cold-copy snapshot of reth datadir + CL DB. ~30-60s downtime per node. 7-day retention. |
| `ops/telos-snapshot.service` + `ops/telos-snapshot.timer` | `/etc/systemd/system/` | systemd service + timer (04:00 UTC daily) for the snapshot. |
| `ops/auto_heal_consensus_quick.sh` | `/usr/local/bin/auto_heal_consensus_quick.sh` | Watches consensus-client journal for `Executor hash mismatch`; on match, restarts reth + CL with cooldown + page-after-N-attempts gate. |
| `ops/telos-autoheal.service` | `/etc/systemd/system/` | systemd service for the autoheal daemon. |

These are running in production today on the validation host. Install procedure for a fresh node is documented in the `RUNBOOK_C10_multi_node_infra.md` in the consensus-client repo's `docs/ops/`.

---

## Where to find what (cross-repo map)

| Topic | Repo + path |
| --- | --- |
| The 2026-04-30 incident source diff | telos-consensus-client `archive/2026-04-30-...` branch |
| Post-mortem | telos-consensus-client `docs/ops/POSTMORTEM_2026-04-30_fork_mvp.md` |
| Production readiness status | telos-consensus-client `docs/ops/PRODUCTION_READINESS_STATUS.md` |
| Multi-node deployment plan + runbook | telos-consensus-client `docs/ops/MULTI_NODE_DEPLOYMENT_PLAN_REFRESH_2026-05-01.md` + `RUNBOOK_C10_multi_node_infra.md` |
| Translator-side fork-handling spec | telos-consensus-client `docs/ops/SPEC_C8_fork_handling.md` |
| Translator-side multi-endpoint canonical spec | telos-consensus-client `docs/ops/SPEC_B7_multi_endpoint_canonical_validation.md` |
| **Reth-side build_state/revm spec** | **this repo** `docs/ops/SPEC_C9_build_state_inverse.md` |
| **Reth-side compare-module investigation** | **this repo** `docs/ops/F2_INVESTIGATION_revm_tevm_compare.md` |
| **Operational tooling (runtime artifacts)** | **this repo** `ops/` |

---

## Versioning

Both repos' `docs/ops/` directories are point-in-time captures as of 2026-05-01. They aren't auto-synced; updates to cross-cutting docs need to be applied to both repos. Where this becomes painful, consider promoting the cross-cutting docs to a separate `telos-reth-ops` repo.
