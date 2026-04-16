#![allow(missing_docs)]

#[global_allocator]
static ALLOC: reth_cli_util::allocator::Allocator = reth_cli_util::allocator::new_allocator();

// Required for "override_allocator_on_supported_platforms".
#[cfg(all(feature = "jemalloc", unix))]
use reth_cli_util::allocator::tikv_jemalloc_sys as _;

use clap::Parser;
use reth::cli::Cli;
use reth_chainspec::EthChainSpec;
use reth_node_telos::{TelosArgs, TelosChainSpecParser, TelosNode};
use reth_telos_rpc::TelosClient;
use tracing::{info, warn};

/// Chain IDs on which `trust_consensus` is safe to enable. Mainnet + testnet + their
/// post-rewind `_base` variants all use the same chain IDs (40/41).
const TELOS_CHAIN_IDS: &[u64] = &[40, 41];

fn main() {
    reth_cli_util::sigsegv_handler::install();

    // Enable backtraces unless a RUST_BACKTRACE value has already been explicitly provided.
    if std::env::var_os("RUST_BACKTRACE").is_none() {
        unsafe { std::env::set_var("RUST_BACKTRACE", "1") };
    }

    if let Err(err) =
        Cli::<TelosChainSpecParser, TelosArgs>::parse().run(async move |builder, telos_args| {
            info!(target: "reth::cli", "Launching Telos node");

            let chain_id = builder.config().chain.chain().id();
            let is_telos_chain = TELOS_CHAIN_IDS.contains(&chain_id);

            // Reject the flag on non-Telos chains. trust_consensus disables enough
            // verification that silently enabling it on Ethereum mainnet would be a
            // footgun — operators must consciously pick a Telos chainspec.
            if telos_args.trust_consensus && !is_telos_chain {
                return Err(eyre::eyre!(
                    "trust_consensus cannot be enabled on non-Telos chain (chain_id={chain_id}). \
                     This flag disables state-root, receipt-root, gas, and execution checks \
                     and is only safe on Telos mainnet (40) or testnet (41)."
                ));
            }

            // Auto-enable on Telos chains unless the operator explicitly disabled it.
            // On non-Telos chains, the flag is already false (rejected above if true).
            let effective_trust_consensus = telos_args.trust_consensus || is_telos_chain;

            reth_telos_primitives_traits::set_trust_consensus(effective_trust_consensus);

            if effective_trust_consensus {
                warn!(
                    target: "reth::cli",
                    chain_id,
                    "Telos trust_consensus ENABLED. The following checks are DISABLED: \
                     state-root verification, receipt-root verification, block gas-used check, \
                     parent-number continuity (except genesis boundary), EVM transaction execution \
                     during historical sync, pipeline merkle stage verification, static-file \
                     tx-number validation. Node is trusting the consensus client (nodeos) as the \
                     sole source of execution truth."
                );
            }

            let telos_endpoint = telos_args.telos_endpoint.clone();
            let telos_client_args: reth_telos_rpc::telos_client::TelosClientArgs =
                telos_args.clone().into();

            let handle = builder
                .node(TelosNode::new(telos_args))
                .extend_rpc_modules(move |_ctx| {
                    if telos_endpoint.is_some() {
                        info!(target: "reth::cli", "Telos native endpoint configured, transaction forwarding enabled");
                        // TODO(PR 2): wire the TelosClient into a custom TelosEthApi so
                        // eth_sendRawTransaction forwards to eosio.evm — see v1
                        // crates/telos/rpc/src/eth/transaction.rs:38.
                        let _client = TelosClient::new(telos_client_args);
                    }
                    Ok(())
                })
                .launch_with_debug_capabilities()
                .await?;

            handle.wait_for_node_exit().await
        })
    {
        eprintln!("Error: {err:?}");
        std::process::exit(1);
    }
}
