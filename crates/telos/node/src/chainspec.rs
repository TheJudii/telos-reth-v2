//! Telos chain specification parser.
//!
//! Extends the Ethereum chain spec parser with Telos-specific chains.

use reth_chainspec::ChainSpec;
use reth_cli::chainspec::ChainSpecParser;
use std::sync::Arc;

/// Telos Mainnet chain spec (chain ID 40).
pub static TELOS_MAINNET: once_cell::sync::Lazy<Arc<ChainSpec>> =
    once_cell::sync::Lazy::new(|| {
        let genesis: alloy_genesis::Genesis =
            serde_json::from_str(include_str!("../res/telos-mainnet.json"))
                .expect("Failed to parse telos-mainnet.json");
        Arc::new(genesis.into())
    });

/// Telos Testnet chain spec (chain ID 41).
pub static TELOS_TESTNET: once_cell::sync::Lazy<Arc<ChainSpec>> =
    once_cell::sync::Lazy::new(|| {
        let genesis: alloy_genesis::Genesis =
            serde_json::from_str(include_str!("../res/telos-testnet.json"))
                .expect("Failed to parse telos-testnet.json");
        Arc::new(genesis.into())
    });

/// Telos Mainnet post-rewind base chain spec (chain ID 40).
///
/// The Telos EVM does not start at block 1. After a rewind, Frontier (and the
/// rest of the pre-Berlin forks) activate at block `180698823`. Use this spec
/// when starting a node from a post-rewind snapshot.
pub static TELOS_MAINNET_BASE: once_cell::sync::Lazy<Arc<ChainSpec>> =
    once_cell::sync::Lazy::new(|| {
        let genesis: alloy_genesis::Genesis =
            serde_json::from_str(include_str!("../res/telos-mainnet-base.json"))
                .expect("Failed to parse telos-mainnet-base.json");
        Arc::new(genesis.into())
    });

/// Telos Testnet post-rewind base chain spec (chain ID 41).
///
/// Frontier activates at block `136393756`. See [`TELOS_MAINNET_BASE`] for context.
pub static TELOS_TESTNET_BASE: once_cell::sync::Lazy<Arc<ChainSpec>> =
    once_cell::sync::Lazy::new(|| {
        let genesis: alloy_genesis::Genesis =
            serde_json::from_str(include_str!("../res/telos-testnet-base.json"))
                .expect("Failed to parse telos-testnet-base.json");
        Arc::new(genesis.into())
    });

/// Chains supported by the Telos node, including standard Ethereum chains.
pub const SUPPORTED_CHAINS: &[&str] = &[
    "mainnet",
    "sepolia",
    "holesky",
    "hoodi",
    "dev",
    "telos-mainnet",
    "telos-testnet",
    "telos-mainnet-base",
    "telos-testnet-base",
];

/// Clap value parser for [`ChainSpec`]s that includes Telos chains.
pub fn telos_chain_value_parser(s: &str) -> eyre::Result<Arc<ChainSpec>, eyre::Error> {
    Ok(match s {
        "telos-mainnet" | "telos" => TELOS_MAINNET.clone(),
        "telos-testnet" => TELOS_TESTNET.clone(),
        "telos-mainnet-base" => TELOS_MAINNET_BASE.clone(),
        "telos-testnet-base" => TELOS_TESTNET_BASE.clone(),
        // Fall back to the Ethereum chain spec parser
        _ => reth_ethereum_cli::chainspec::chain_value_parser(s)?,
    })
}

/// Telos chain specification parser.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct TelosChainSpecParser;

impl ChainSpecParser for TelosChainSpecParser {
    type ChainSpec = ChainSpec;

    const SUPPORTED_CHAINS: &'static [&'static str] = SUPPORTED_CHAINS;

    fn parse(s: &str) -> eyre::Result<Arc<ChainSpec>> {
        telos_chain_value_parser(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::{b256, B256};

    /// v1 genesis hash: `crates/primitives-traits/src/constants/mod.rs::TEVMMAINNET_GENESIS_HASH`.
    const TEVMMAINNET_GENESIS_HASH: B256 =
        b256!("36fe7024b760365e3970b7b403e161811c1e626edd68460272fcdfa276272563");

    /// v1 genesis hash: `crates/primitives-traits/src/constants/mod.rs::TEVMTESTNET_GENESIS_HASH`.
    const TEVMTESTNET_GENESIS_HASH: B256 =
        b256!("b25034033c9ca7a40e879ddcc29cf69071a22df06688b5fe8cc2d68b4e0528f9");

    /// v1 genesis hash: `crates/primitives-traits/src/constants/mod.rs::TEVMMAINNET_BASE_GENESIS_HASH`.
    const TEVMMAINNET_BASE_GENESIS_HASH: B256 =
        b256!("757720a8e51c63ef1d4f907d6569dacaa965e91c2661345902de18af11f81063");

    /// v1 genesis hash: `crates/primitives-traits/src/constants/mod.rs::TEVMTESTNET_BASE_GENESIS_HASH`.
    const TEVMTESTNET_BASE_GENESIS_HASH: B256 =
        b256!("a6da3143bdeab454a923ac47589700ebe75d734f26e1f9201caa9b7268045d02");

    #[test]
    fn parse_telos_chains() {
        assert!(TelosChainSpecParser::parse("telos-mainnet").is_ok());
        assert!(TelosChainSpecParser::parse("telos-testnet").is_ok());
        assert!(TelosChainSpecParser::parse("telos").is_ok());
        assert!(TelosChainSpecParser::parse("telos-mainnet-base").is_ok());
        assert!(TelosChainSpecParser::parse("telos-testnet-base").is_ok());
    }

    #[test]
    fn parse_standard_chains() {
        for &chain in &["mainnet", "sepolia", "holesky", "hoodi", "dev"] {
            assert!(TelosChainSpecParser::parse(chain).is_ok());
        }
    }

    #[test]
    fn telos_mainnet_chain_id() {
        let spec = TelosChainSpecParser::parse("telos-mainnet").unwrap();
        assert_eq!(spec.chain().id(), 40);
    }

    #[test]
    fn telos_testnet_chain_id() {
        let spec = TelosChainSpecParser::parse("telos-testnet").unwrap();
        assert_eq!(spec.chain().id(), 41);
    }

    #[test]
    fn telos_mainnet_base_chain_id() {
        let spec = TelosChainSpecParser::parse("telos-mainnet-base").unwrap();
        assert_eq!(spec.chain().id(), 40);
    }

    #[test]
    fn telos_testnet_base_chain_id() {
        let spec = TelosChainSpecParser::parse("telos-testnet-base").unwrap();
        assert_eq!(spec.chain().id(), 41);
    }

    /// Peering requires the genesis hash to match the value baked into v1 nodes.
    /// If this test fails, the genesis JSON drifted — fix the JSON, not the test.
    #[test]
    fn test_mainnet_genesis_hash() {
        assert_eq!(TELOS_MAINNET.genesis_hash(), TEVMMAINNET_GENESIS_HASH);
    }

    #[test]
    fn test_testnet_genesis_hash() {
        assert_eq!(TELOS_TESTNET.genesis_hash(), TEVMTESTNET_GENESIS_HASH);
    }

    #[test]
    fn test_mainnet_base_genesis_hash() {
        assert_eq!(TELOS_MAINNET_BASE.genesis_hash(), TEVMMAINNET_BASE_GENESIS_HASH);
    }

    #[test]
    fn test_testnet_base_genesis_hash() {
        assert_eq!(TELOS_TESTNET_BASE.genesis_hash(), TEVMTESTNET_BASE_GENESIS_HASH);
    }
}
