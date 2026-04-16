//! clap [Args](clap::Args) for Telos configuration

use crate::DEFAULT_MAX_EXECUTE_BLOCK_BATCH_SIZE;
use reth_telos_rpc::telos_client::TelosClientArgs;
use std::path::PathBuf;

/// Telos CLI arguments
#[derive(Debug, Clone, PartialEq, Eq, clap::Args)]
#[clap(next_help_heading = "Telos")]
pub struct TelosArgs {
    /// TelosZero endpoint to use for API calls (send_transaction, get gas price from table)
    #[arg(long = "telos.telos_endpoint", value_name = "HTTP_URL")]
    pub telos_endpoint: Option<String>,

    /// Signer account name
    #[arg(long = "telos.signer_account")]
    pub signer_account: Option<String>,

    /// Signer permission name
    #[arg(long = "telos.signer_permission")]
    pub signer_permission: Option<String>,

    /// Signer private key (DEPRECATED — prefer `--telos.signer_key_file`).
    ///
    /// Passing the private key as a CLI argument leaks it into `/proc/<pid>/cmdline`
    /// and shell history. Use `--telos.signer_key_file` in production.
    #[arg(long = "telos.signer_key")]
    pub signer_key: Option<String>,

    /// Path to a file containing the signer private key.
    /// Preferred over --telos.signer_key for operational security.
    #[arg(long = "telos.signer_key_file", value_name = "PATH", conflicts_with = "signer_key")]
    pub signer_key_file: Option<PathBuf>,

    /// Seconds to cache gas price
    #[arg(long = "telos.gas_cache_seconds")]
    pub gas_cache_seconds: Option<u32>,

    /// Maximum number of blocks to execute sequentially in a batch.
    #[arg(long = "engine.max-execute-block-batch-size", default_value_t = DEFAULT_MAX_EXECUTE_BLOCK_BATCH_SIZE)]
    pub max_execute_block_batch_size: usize,

    /// Block delta between native and EVM
    #[arg(long = "telos.block_delta")]
    pub block_delta: Option<u32>,

    /// Trust consensus client execution results and skip reth's independent verification.
    ///
    /// When enabled, reth disables state-root / receipt-root / gas verification and trusts
    /// the consensus client (nodeos) as the sole source of execution truth. This is only
    /// safe on Telos chains (mainnet=40, testnet=41) where real state lives in nodeos and
    /// EVM header root fields are empty-trie placeholders.
    ///
    /// Defaults to `false`. When running on a Telos chain (`--chain telos-mainnet` /
    /// `telos-testnet` / their `-base` variants), the node auto-enables this flag; pass
    /// `--telos.trust_consensus=false` to opt out. Enabling this flag on a non-Telos
    /// chain will cause the node to refuse to start.
    #[arg(long = "telos.trust_consensus", default_value_t = false, action = clap::ArgAction::Set)]
    pub trust_consensus: bool,
}

impl Default for TelosArgs {
    fn default() -> Self {
        Self {
            telos_endpoint: None,
            signer_account: None,
            signer_permission: None,
            signer_key: None,
            signer_key_file: None,
            gas_cache_seconds: None,
            max_execute_block_batch_size: DEFAULT_MAX_EXECUTE_BLOCK_BATCH_SIZE,
            block_delta: None,
            trust_consensus: false,
        }
    }
}

impl From<TelosArgs> for TelosClientArgs {
    fn from(args: TelosArgs) -> Self {
        // Prefer signer_key_file over the CLI-string signer_key; emit a deprecation warning
        // if only the CLI form is provided.
        let signer_key = match (args.signer_key_file, args.signer_key) {
            (Some(path), _) => match std::fs::read_to_string(&path) {
                Ok(contents) => Some(contents.trim().to_string()),
                Err(err) => {
                    tracing::error!(
                        target: "reth::cli",
                        ?path,
                        %err,
                        "failed to read --telos.signer_key_file; TelosClient will be missing signer_key",
                    );
                    None
                }
            },
            (None, Some(key)) => {
                tracing::warn!(
                    target: "reth::cli",
                    "--telos.signer_key is deprecated; prefer --telos.signer_key_file to avoid leaking the key into /proc/<pid>/cmdline",
                );
                Some(key)
            }
            (None, None) => None,
        };

        TelosClientArgs {
            telos_endpoint: args.telos_endpoint,
            signer_account: args.signer_account,
            signer_permission: args.signer_permission,
            signer_key,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::{Args, Parser};

    /// A helper type to parse Args more easily
    #[derive(Parser)]
    struct CommandParser<T: Args> {
        #[clap(flatten)]
        args: T,
    }

    #[test]
    fn test_parse_telos_args() {
        let default_args = TelosArgs::default();
        let args = CommandParser::<TelosArgs>::parse_from(["reth"]).args;
        assert_eq!(args, default_args);
    }

    #[test]
    fn test_trust_consensus_defaults_to_false() {
        let args = CommandParser::<TelosArgs>::parse_from(["reth"]).args;
        assert!(!args.trust_consensus);
    }

    #[test]
    fn test_signer_key_and_key_file_are_mutually_exclusive() {
        let result = CommandParser::<TelosArgs>::try_parse_from([
            "reth",
            "--telos.signer_key",
            "abc",
            "--telos.signer_key_file",
            "/tmp/key",
        ]);
        assert!(result.is_err(), "signer_key and signer_key_file must conflict");
    }
}
