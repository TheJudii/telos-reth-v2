//! Telos chain specification parser.
//!
//! Extends the Ethereum chain spec parser with Telos-specific chains.

use reth_chainspec::ChainSpec;
use reth_cli::chainspec::ChainSpecParser;
use std::sync::Arc;

/// Telos Mainnet chain spec (chain ID 40)
pub static TELOS_MAINNET: once_cell::sync::Lazy<Arc<ChainSpec>> =
    once_cell::sync::Lazy::new(|| {
        let genesis: alloy_genesis::Genesis =
            serde_json::from_str(include_str!("../res/telos-mainnet.json"))
                .expect("Failed to parse telos-mainnet.json");
        Arc::new(genesis.into())
    });

/// Telos Testnet chain spec (chain ID 41)
pub static TELOS_TESTNET: once_cell::sync::Lazy<Arc<ChainSpec>> =
    once_cell::sync::Lazy::new(|| {
        let genesis: alloy_genesis::Genesis =
            serde_json::from_str(include_str!("../res/telos-testnet.json"))
                .expect("Failed to parse telos-testnet.json");
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
];

/// Clap value parser for [`ChainSpec`]s that includes Telos chains.
pub fn telos_chain_value_parser(s: &str) -> eyre::Result<Arc<ChainSpec>, eyre::Error> {
    Ok(match s {
        "telos-mainnet" | "telos" => TELOS_MAINNET.clone(),
        "telos-testnet" => TELOS_TESTNET.clone(),
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

    #[test]
    fn parse_telos_chains() {
        assert!(TelosChainSpecParser::parse("telos-mainnet").is_ok());
        assert!(TelosChainSpecParser::parse("telos-testnet").is_ok());
        assert!(TelosChainSpecParser::parse("telos").is_ok());
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
}
