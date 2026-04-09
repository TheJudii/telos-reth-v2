#![allow(missing_docs)]

#[global_allocator]
static ALLOC: reth_cli_util::allocator::Allocator = reth_cli_util::allocator::new_allocator();

// Required for "override_allocator_on_supported_platforms".
#[cfg(all(feature = "jemalloc", unix))]
use reth_cli_util::allocator::tikv_jemalloc_sys as _;

use clap::Parser;
use reth::cli::Cli;
use reth_node_telos::{TelosArgs, TelosChainSpecParser, TelosNode};
use reth_telos_rpc::TelosClient;
use tracing::info;

fn main() {
    reth_cli_util::sigsegv_handler::install();

    // Enable backtraces unless a RUST_BACKTRACE value has already been explicitly provided.
    if std::env::var_os("RUST_BACKTRACE").is_none() {
        unsafe { std::env::set_var("RUST_BACKTRACE", "1") };
    }

    if let Err(err) =
        Cli::<TelosChainSpecParser, TelosArgs>::parse().run(async move |builder, telos_args| {
            info!(target: "reth::cli", "Launching Telos node");

            let telos_endpoint = telos_args.telos_endpoint.clone();
            let telos_client_args: reth_telos_rpc::telos_client::TelosClientArgs =
                telos_args.clone().into();

            let handle = builder
                .node(TelosNode::new(telos_args))
                .extend_rpc_modules(move |_ctx| {
                    if telos_endpoint.is_some() {
                        info!(target: "reth::cli", "Telos native endpoint configured, transaction forwarding enabled");
                        // The TelosClient is available for RPC extensions that need
                        // to forward transactions to the native chain
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
