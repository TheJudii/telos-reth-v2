//! Crate for Telos specific primitive traits

#![doc(
    html_logo_url = "https://raw.githubusercontent.com/paradigmxyz/reth/main/assets/reth-docs.png",
    html_favicon_url = "https://avatars0.githubusercontent.com/u/97369466?s=256",
    issue_tracker_base_url = "https://github.com/telosnetwork/telos-reth/issues/"
)]
#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]

use alloy_primitives::U256;
use serde::{Deserialize, Serialize};

use std::sync::atomic::{AtomicBool, Ordering};

/// Global flag: when true, reth trusts execution results from the consensus client (nodeos)
/// instead of re-executing and re-verifying the state trie. Default: false.
///
/// Set once at startup from the `--telos.trust_consensus` CLI flag.
static TRUST_CONSENSUS: AtomicBool = AtomicBool::new(false);

/// Set the global trust_consensus flag (called once at startup).
pub fn set_trust_consensus(v: bool) {
    TRUST_CONSENSUS.store(v, Ordering::Relaxed);
}

/// Returns true if reth should trust consensus client execution results.
pub fn trust_consensus() -> bool {
    TRUST_CONSENSUS.load(Ordering::Relaxed)
}

/// Telos block extension fields, included in Headers table as part of Header
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TelosBlockExtension {
    /// Initial gas price for this block
    pub starting_gas_price: U256,
    /// Initial revision number for this block
    pub starting_revision_number: u64,
    /// Changed gas price for this block
    pub gas_price_change: Option<GasPrice>,
    /// Changed revision number for this block
    pub revision_change: Option<Revision>,
}

impl TelosBlockExtension {
    /// Create a new `TelosBlockExtension` using a parent extension to fetch the starting
    /// price/revision plus changes for the current block
    pub fn from_parent_and_changes(
        parent: &Self,
        gas_price_change: Option<(u64, U256)>,
        revision_change: Option<(u64, u64)>,
    ) -> Self {
        let mut starting_gas_price = parent.get_last_gas_price();
        let mut starting_revision_number = parent.get_last_revision();
        let gas_price_change = if let Some(price_change) = gas_price_change {
            if price_change.0 > 0 {
                Some(GasPrice { height: price_change.0, price: price_change.1 })
            } else {
                starting_gas_price = price_change.1;
                None
            }
        } else {
            None
        };

        let revision_change = if let Some(revision) = revision_change {
            if revision.0 > 0 {
                Some(Revision { height: revision.0, revision: revision.1 })
            } else {
                starting_revision_number = revision.1;
                None
            }
        } else {
            None
        };

        Self { starting_gas_price, starting_revision_number, gas_price_change, revision_change }
    }

    /// Create a new Telos block extension for a child block
    pub fn to_child(&self) -> Self {
        Self {
            starting_gas_price: self.get_last_gas_price(),
            starting_revision_number: self.get_last_revision(),
            gas_price_change: None,
            revision_change: None,
        }
    }

    /// Get the ending gas price of this block
    pub fn get_last_gas_price(&self) -> U256 {
        self.gas_price_change.as_ref().map_or(self.starting_gas_price, |c| c.price)
    }

    /// Get the ending revision number of this block
    pub fn get_last_revision(&self) -> u64 {
        self.revision_change.as_ref().map_or(self.starting_revision_number, |c| c.revision)
    }

    /// Get `TelosTxEnv` at a given transaction index
    pub fn tx_env_at(&self, height: u64) -> TelosTxEnv {
        let gas_price =
            if self.gas_price_change.as_ref().is_some_and(|c| c.height <= height) {
                self.gas_price_change.as_ref().unwrap().price
            } else {
                self.starting_gas_price
            };

        let revision =
            if self.revision_change.as_ref().is_some_and(|c| c.height <= height) {
                self.revision_change.as_ref().unwrap().revision
            } else {
                self.starting_revision_number
            };

        TelosTxEnv { gas_price, revision }
    }
}

/// Telos transaction environment data
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TelosTxEnv {
    /// Gas price for this transaction
    pub gas_price: U256,
    /// Revision number for this transaction
    pub revision: u64,
}

/// Telos gas price
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GasPrice {
    /// Transaction height
    pub height: u64,
    /// Value
    pub price: U256,
}

/// Telos revision number
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Revision {
    /// Transaction height
    pub height: u64,
    /// Revision
    pub revision: u64,
}
