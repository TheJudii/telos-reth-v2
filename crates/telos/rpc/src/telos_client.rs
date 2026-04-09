//! Telos native chain client for forwarding transactions

use std::sync::Arc;
use std::time::Duration;

use alloy_primitives::Bytes;
use reth_rpc_eth_types::EthApiError;
use serde::{Deserialize, Serialize};
use tracing::{debug, error, warn};

/// Arguments for constructing a [`TelosClient`]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TelosClientArgs {
    /// Telos native endpoint to forward transactions to
    pub telos_endpoint: Option<String>,
    /// Signer account name
    pub signer_account: Option<String>,
    /// Signer permission name
    pub signer_permission: Option<String>,
    /// Signer private key
    pub signer_key: Option<String>,
}

/// A client to interact with a Telos native node.
///
/// Used primarily for forwarding `eth_sendRawTransaction` calls
/// to the Telos native network for inclusion in blocks.
#[derive(Debug, Clone)]
pub struct TelosClient {
    inner: Arc<TelosClientInner>,
}

#[derive(Debug, Clone)]
struct TelosClientInner {
    endpoint: String,
    signer_account: String,
    signer_permission: String,
    signer_key: String,
    http_client: reqwest::Client,
}

impl TelosClient {
    /// Creates a new [`TelosClient`].
    ///
    /// # Panics
    /// Panics if required args (endpoint, signer fields) are not provided.
    pub fn new(args: TelosClientArgs) -> Self {
        let endpoint = args
            .telos_endpoint
            .expect("telos_endpoint is required for TelosClient");
        let signer_account = args
            .signer_account
            .expect("signer_account is required for TelosClient");
        let signer_permission = args
            .signer_permission
            .expect("signer_permission is required for TelosClient");
        let signer_key = args
            .signer_key
            .expect("signer_key is required for TelosClient");

        let http_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Failed to build HTTP client");

        Self {
            inner: Arc::new(TelosClientInner {
                endpoint,
                signer_account,
                signer_permission,
                signer_key,
                http_client,
            }),
        }
    }

    /// Returns the configured Telos native endpoint
    pub fn endpoint(&self) -> &str {
        &self.inner.endpoint
    }

    /// Returns the signer account name
    pub fn signer_account(&self) -> &str {
        &self.inner.signer_account
    }

    /// Returns the signer permission
    pub fn signer_permission(&self) -> &str {
        &self.inner.signer_permission
    }

    /// Sends a raw EVM transaction to the Telos native network for inclusion in a block.
    ///
    /// The raw transaction bytes are wrapped in a `raw` action to the `eosio.evm` contract
    /// and submitted via the native chain's `send_transaction2` endpoint.
    pub async fn send_to_telos(&self, tx: &[u8]) -> Result<(), EthApiError> {
        let payload = serde_json::json!({
            "transaction": {
                "actions": [{
                    "account": "eosio.evm",
                    "name": "raw",
                    "authorization": [{
                        "actor": self.inner.signer_account,
                        "permission": self.inner.signer_permission,
                    }],
                    "data": {
                        "ram_payer": "eosio.evm",
                        "tx": hex::encode(tx),
                        "estimate_gas": false,
                        "sender": null
                    }
                }]
            },
            "return_failure_trace": true,
            "retry_trx": true,
            "retry_trx_num_blocks": 2
        });

        // Retry with exponential backoff
        let mut backoff_ms = 2u64;
        let max_retries = 12;
        for attempt in 0..max_retries {
            match self
                .inner
                .http_client
                .post(format!("{}/v1/chain/send_transaction2", self.inner.endpoint))
                .json(&payload)
                .send()
                .await
            {
                Ok(response) => {
                    if response.status().is_success() {
                        debug!("Transaction sent to Telos native (attempt {})", attempt + 1);
                        return Ok(());
                    }
                    let status = response.status();
                    let body = response.text().await.unwrap_or_default();
                    if attempt == max_retries - 1 {
                        error!("Telos native rejected transaction: {} {}", status, body);
                        return Err(EthApiError::EvmCustom(format!(
                            "Telos native error: {body}"
                        )));
                    }
                    warn!(
                        "Telos native returned {}, retrying (attempt {}/{})",
                        status,
                        attempt + 1,
                        max_retries
                    );
                }
                Err(err) => {
                    if attempt == max_retries - 1 {
                        error!("Failed to send transaction to Telos: {:?}", err);
                        return Err(EthApiError::EvmCustom(format!(
                            "Telos network error: {err}"
                        )));
                    }
                    warn!(
                        "Network error sending to Telos, retrying (attempt {}/{}): {}",
                        attempt + 1,
                        max_retries,
                        err
                    );
                }
            }
            tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
            backoff_ms = (backoff_ms * 2).min(4096);
        }

        unreachable!()
    }
}
