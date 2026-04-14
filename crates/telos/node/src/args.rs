//! clap [Args](clap::Args) for Telos configuration

use crate::DEFAULT_MAX_EXECUTE_BLOCK_BATCH_SIZE;
use reth_telos_rpc::telos_client::TelosClientArgs;

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

    /// Signer private key
    #[arg(long = "telos.signer_key")]
    pub signer_key: Option<String>,

    /// Seconds to cache gas price
    #[arg(long = "telos.gas_cache_seconds")]
    pub gas_cache_seconds: Option<u32>,

    /// Maximum number of blocks to execute sequentially in a batch.
    #[arg(long = "engine.max-execute-block-batch-size", default_value_t = DEFAULT_MAX_EXECUTE_BLOCK_BATCH_SIZE)]
    pub max_execute_block_batch_size: usize,

    /// Block delta between native and EVM
    #[arg(long = "telos.block_delta")]
    pub block_delta: Option<u32>,

    /// Trust consensus client execution results and skip state root verification.
    /// Required for Telos testnet/mainnet where real state lives in nodeos and EVM
    /// header root fields are empty-trie placeholders. Default: true.
    #[arg(long = "telos.trust_consensus", default_value_t = true, action = clap::ArgAction::Set)]
    pub trust_consensus: bool,

    /// Build EVM state while keeping trust_consensus enabled. Executes transactions and builds state
    /// without validating state roots, allowing hybrid mode for historical block analysis.




    #[arg(long = "telos.build_state", default_value_t = false)]
    pub build_state: bool,
}

impl Default for TelosArgs {
    fn default() -> Self {
        Self {
            telos_endpoint: None,
            signer_account: None,
            signer_permission: None,
            signer_key: None,
            gas_cache_seconds: None,
            max_execute_block_batch_size: DEFAULT_MAX_EXECUTE_BLOCK_BATCH_SIZE,
            block_delta: None,
            trust_consensus: true,
            build_state: false,
        }
    }
}

impl From<TelosArgs> for TelosClientArgs {
    fn from(args: TelosArgs) -> Self {
        TelosClientArgs {
            telos_endpoint: args.telos_endpoint,
            signer_account: args.signer_account,
            signer_permission: args.signer_permission,
            signer_key: args.signer_key,
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
}
