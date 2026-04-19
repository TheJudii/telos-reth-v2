//! Telos native chain client for forwarding transactions.

use std::{sync::Arc, time::Duration};

use alloy_primitives::{keccak256, Bytes, B256};
use jsonrpsee::server::RpcModule;
use jsonrpsee_types::{ErrorObject, ErrorObjectOwned};
use reth_rpc_eth_types::EthApiError;
use secp256k1::SecretKey;
use serde::{Deserialize, Serialize};
use tracing::{debug, error, info, warn};

use crate::antelope::{
    self, name_to_u64, now_plus, ref_block_num, ref_block_prefix, serialize_raw_action_data,
    sig_digest, sign_k1_canonical, wif_to_secret_key, PackedAction, PackedTransaction,
};

/// Arguments for constructing a [`TelosClient`].
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TelosClientArgs {
    /// HTTP endpoint for nodeos (e.g. `http://127.0.0.1:8888`).
    pub telos_endpoint: Option<String>,
    /// Antelope account name that signs forwarded transactions (e.g. `rpc.evm`).
    pub signer_account: Option<String>,
    /// Permission level used by the signer (e.g. `forward` or `active`).
    pub signer_permission: Option<String>,
    /// WIF-encoded signer private key (`5K...` or `5J...`).
    pub signer_key: Option<String>,
}

/// A client that forwards signed Ethereum transactions to the Telos native chain
/// by wrapping them in an `eosio.evm::raw` action and submitting a signed Antelope
/// transaction to `/v1/chain/send_transaction2`.
#[derive(Debug, Clone)]
pub struct TelosClient {
    inner: Arc<TelosClientInner>,
}

#[derive(Debug)]
struct TelosClientInner {
    endpoint: String,
    signer_actor: u64,
    signer_permission: u64,
    ram_payer: u64,
    contract_account: u64,
    action_name: u64,
    secret_key: SecretKey,
    http_client: reqwest::Client,
}

#[derive(Debug, Deserialize)]
struct GetInfoResponse {
    chain_id: String,
    last_irreversible_block_num: u32,
    last_irreversible_block_id: String,
}

impl TelosClient {
    /// Creates a new [`TelosClient`]. Panics on missing or malformed required args.
    pub fn new(args: TelosClientArgs) -> Self {
        let endpoint = args.telos_endpoint.expect("telos_endpoint is required for TelosClient");
        let signer_account_str =
            args.signer_account.expect("signer_account is required for TelosClient");
        let signer_permission_str =
            args.signer_permission.expect("signer_permission is required for TelosClient");
        let signer_key_str = args.signer_key.expect("signer_key is required for TelosClient");

        let signer_actor =
            name_to_u64(&signer_account_str).expect("invalid signer_account name encoding");
        let signer_permission_u64 =
            name_to_u64(&signer_permission_str).expect("invalid signer_permission name encoding");
        let ram_payer = name_to_u64("eosio.evm").expect("eosio.evm name encoding");
        let contract_account = ram_payer;
        let action_name = name_to_u64("raw").expect("raw name encoding");
        let secret_key = wif_to_secret_key(&signer_key_str).expect("invalid signer_key WIF");

        let http_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Failed to build HTTP client");

        Self {
            inner: Arc::new(TelosClientInner {
                endpoint,
                signer_actor,
                signer_permission: signer_permission_u64,
                ram_payer,
                contract_account,
                action_name,
                secret_key,
                http_client,
            }),
        }
    }

    /// Returns the nodeos HTTP endpoint this client was configured with.
    pub fn endpoint(&self) -> &str {
        &self.inner.endpoint
    }

    /// Sign + submit a raw EVM transaction through `eosio.evm::raw`.
    ///
    /// 1. Fetch `get_info` for `chain_id` and a LIB block for TAPOS.
    /// 2. Build the action + packed transaction.
    /// 3. sha256(`chain_id` || `packed_trx` || `zero_cfa_hash`) → digest.
    /// 4. K1 canonical sign.
    /// 5. POST to `/v1/chain/send_transaction2`.
    pub async fn send_to_telos(&self, tx: &[u8]) -> Result<(), EthApiError> {
        let max_retries = 6;
        let mut backoff_ms = 50u64;

        for attempt in 0..max_retries {
            match self.submit_once(tx).await {
                Ok(()) => {
                    debug!(attempt, "forwarded tx to Telos native");
                    return Ok(());
                }
                Err(err) => {
                    if attempt == max_retries - 1 {
                        error!(error = %err, "giving up forwarding tx to Telos native");
                        return Err(EthApiError::EvmCustom(format!("Telos forward error: {err}")));
                    }
                    warn!(
                        attempt,
                        error = %err,
                        "forward failed, retrying"
                    );
                }
            }
            tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
            backoff_ms = (backoff_ms * 2).min(2000);
        }
        unreachable!()
    }

    async fn submit_once(&self, tx: &[u8]) -> Result<(), antelope::AntelopeError> {
        let info = self.get_info().await?;

        // Parse chain_id / block_id as 32-byte digests.
        let chain_id_bytes = hex::decode(&info.chain_id)?;
        if chain_id_bytes.len() != 32 {
            return Err(antelope::AntelopeError::BadBlockId);
        }
        let mut chain_id = [0u8; 32];
        chain_id.copy_from_slice(&chain_id_bytes);
        let chain_id = B256::from(chain_id);

        let block_id_bytes = hex::decode(&info.last_irreversible_block_id)?;
        if block_id_bytes.len() != 32 {
            return Err(antelope::AntelopeError::BadBlockId);
        }
        let mut block_id_arr = [0u8; 32];
        block_id_arr.copy_from_slice(&block_id_bytes);
        let block_id = B256::from(block_id_arr);

        // Build action data.
        let action_data = serialize_raw_action_data(self.inner.ram_payer, tx, false, None);

        let action = PackedAction {
            account: self.inner.contract_account,
            name: self.inner.action_name,
            authorization: vec![(self.inner.signer_actor, self.inner.signer_permission)],
            data: action_data,
        };

        let packed = PackedTransaction {
            expiration: now_plus(60),
            ref_block_num: ref_block_num(info.last_irreversible_block_num),
            ref_block_prefix: ref_block_prefix(&block_id),
            max_net_usage_words: 0,
            max_cpu_usage_ms: 0,
            delay_sec: 0,
            actions: vec![action],
        };
        let packed_bytes = packed.serialize();

        let digest = sig_digest(&chain_id, &packed_bytes);
        let signature = sign_k1_canonical(&self.inner.secret_key, &digest)?;

        let payload = serde_json::json!({
            "signatures": [signature],
            "compression": "none",
            "packed_context_free_data": "",
            "packed_trx": hex::encode(&packed_bytes),
        });

        let url = format!("{}/v1/chain/send_transaction2", self.inner.endpoint);
        let body = serde_json::json!({
            "return_failure_trace": true,
            "retry_trx": true,
            "retry_trx_num_blocks": 2,
            "transaction": payload,
        });

        let resp = self.inner.http_client.post(&url).json(&body).send().await?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(antelope::AntelopeError::Nodeos { status: status.as_u16(), body: text });
        }
        Ok(())
    }

    /// Build a jsonrpsee RPC module that overrides `eth_sendRawTransaction` to
    /// forward the raw transaction to Telos native via [`Self::send_to_telos`].
    ///
    /// The handler decodes the raw bytes, computes the EVM transaction hash, and
    /// returns it synchronously after the native submission succeeds. It does NOT
    /// insert the transaction into reth's local pool — blocks produced by nodeos
    /// flow back through the consensus client and will land the tx naturally.
    pub fn build_forwarder_module(&self) -> Result<RpcModule<()>, ErrorObjectOwned> {
        let client = self.clone();
        let mut module = RpcModule::new(());
        module
            .register_async_method("eth_sendRawTransaction", move |params, _ctx, _ext| {
                let client = client.clone();
                async move {
                    let (bytes,): (Bytes,) = params.parse().map_err(|e| {
                        ErrorObject::owned(
                            -32602,
                            format!("invalid params: {e}"),
                            None::<()>,
                        )
                    })?;
                    let hash: B256 = keccak256(&bytes);
                    info!(target: "telos::forward", tx_hash = %hash, bytes = bytes.len(), "forwarding tx to Telos native");
                    if let Err(err) = client.send_to_telos(&bytes).await {
                        error!(target: "telos::forward", error = %err, tx_hash = %hash, "forward failed");
                        return Err(err.into_rpc_err());
                    }
                    Ok::<B256, ErrorObject<'static>>(hash)
                }
            })
            .map_err(|e| {
                ErrorObject::owned(-32603, format!("register method: {e}"), None::<()>)
            })?;
        Ok(module)
    }

    async fn get_info(&self) -> Result<GetInfoResponse, antelope::AntelopeError> {
        let url = format!("{}/v1/chain/get_info", self.inner.endpoint);
        let resp = self.inner.http_client.post(&url).send().await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(antelope::AntelopeError::Nodeos { status: status.as_u16(), body });
        }
        let info: GetInfoResponse = resp.json().await?;
        Ok(info)
    }
}
