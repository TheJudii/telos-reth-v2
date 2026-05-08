//! Telos native chain client for forwarding transactions.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use alloy_primitives::{keccak256, Bytes, B256, U256};
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

/// Default gas-price cache TTL (seconds) when `--telos.gas_cache_seconds` is not set.
/// 8 seconds chosen because the eosio.evm config table is updated by an on-chain action
/// at most once every few minutes; 8s gives sub-block freshness without hammering nodeos.
const DEFAULT_GAS_CACHE_SECONDS: u32 = 8;
const DEFAULT_TX_RETRY_BLOCKS: u32 = 120;

/// `eth_maxPriorityFeePerGas` constant returned by the canonical Telos RPC.
/// 1 gwei = 0x3b9aca00. Telos has no priority-fee market — transactions pay only
/// `gas_price` from the eosio.evm config — but the canonical RPC returns 1 gwei
/// to satisfy EIP-1559 wallets. We mirror that for parity.
const TELOS_MAX_PRIORITY_FEE_PER_GAS_WEI: u64 = 1_000_000_000;

/// Arguments for constructing a [`TelosClient`].
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TelosClientArgs {
    pub telos_endpoint: Option<String>,
    pub signer_account: Option<String>,
    pub signer_permission: Option<String>,
    pub signer_key: Option<String>,
    /// Seconds to cache the gas-price reading from the `eosio.evm` config table.
    /// Defaults to [`DEFAULT_GAS_CACHE_SECONDS`] when unset.
    pub gas_cache_seconds: Option<u32>,
    /// Number of native blocks nodeos should keep retrying a forwarded transaction.
    /// Defaults to [`DEFAULT_TX_RETRY_BLOCKS`] when unset.
    pub tx_retry_blocks: Option<u32>,
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
    gas_cache_seconds: u32,
    tx_retry_blocks: u32,
    gas_price_cache: Mutex<Option<(Instant, U256)>>,
}

#[derive(Debug, Deserialize)]
struct GetInfoResponse {
    chain_id: String,
    last_irreversible_block_num: u32,
    last_irreversible_block_id: String,
}

#[derive(Debug, Deserialize)]
struct GetTableRowsResponse {
    rows: Vec<EvmConfigRow>,
}

/// Subset of the `eosio.evm` config-table row we care about. The contract
/// stores `gas_price` as a hex-encoded `uint256` string (e.g. `"4c68cd444de"`)
/// representing wei.
#[derive(Debug, Deserialize)]
struct EvmConfigRow {
    gas_price: String,
}

impl TelosClient {
    /// Creates a new [`TelosClient`]. Panics on missing or malformed required args.
    pub fn new(args: TelosClientArgs) -> Self {
        let endpoint = args
            .telos_endpoint
            .expect("telos_endpoint is required for TelosClient");
        let signer_account_str = args
            .signer_account
            .expect("signer_account is required for TelosClient");
        let signer_permission_str = args
            .signer_permission
            .expect("signer_permission is required for TelosClient");
        let signer_key_str = args
            .signer_key
            .expect("signer_key is required for TelosClient");
        let gas_cache_seconds = args.gas_cache_seconds.unwrap_or(DEFAULT_GAS_CACHE_SECONDS);
        let tx_retry_blocks = args.tx_retry_blocks.unwrap_or(DEFAULT_TX_RETRY_BLOCKS);

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
                gas_cache_seconds,
                tx_retry_blocks,
                gas_price_cache: Mutex::new(None),
            }),
        }
    }

    pub fn endpoint(&self) -> &str {
        &self.inner.endpoint
    }

    /// Sign + submit a raw EVM transaction through `eosio.evm::raw`.
    ///
    /// 1. Fetch `get_info` for `chain_id` and a LIB block for TAPOS.
    /// 2. Build the action + packed transaction.
    /// 3. sha256(chain_id || packed_trx || zero_cfa_hash) → digest.
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
            "retry_trx_num_blocks": self.inner.tx_retry_blocks,
            "transaction": payload,
        });

        let resp = self.inner.http_client.post(&url).json(&body).send().await?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(antelope::AntelopeError::Nodeos {
                status: status.as_u16(),
                body: text,
            });
        }
        Ok(())
    }

    /// Build a jsonrpsee RPC module that overrides:
    ///
    /// - `eth_sendRawTransaction` — forwards the raw transaction to Telos native
    ///   via [`send_to_telos`]. The handler decodes the raw bytes, computes the
    ///   EVM transaction hash, and returns it synchronously after the native
    ///   submission succeeds. It does NOT insert the transaction into reth's
    ///   local pool — blocks produced by nodeos flow back through the consensus
    ///   client and will land the tx naturally.
    /// - `eth_gasPrice` — returns the canonical gas price from the `eosio.evm`
    ///   config table on-chain (cached for `gas_cache_seconds`). Without this
    ///   override, the default reth oracle samples recent block transactions and
    ///   returns 0 on Telos because empty 0.5s blocks dominate the sample window.
    ///   Wallets and SDKs depend on a non-zero value to construct legacy txs.
    /// - `eth_maxPriorityFeePerGas` — returns 1 gwei to mirror canonical RPC.
    ///   Telos has no priority-fee market; transactions pay only `gas_price` from
    ///   the config table. EIP-1559 wallets nonetheless query this method and a
    ///   0 reply makes them refuse to broadcast.
    pub fn build_forwarder_module(&self) -> Result<RpcModule<()>, ErrorObjectOwned> {
        let mut module = RpcModule::new(());

        // eth_sendRawTransaction — forward to Telos native.
        let forward_client = self.clone();
        module
            .register_async_method("eth_sendRawTransaction", move |params, _ctx, _ext| {
                let client = forward_client.clone();
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

        // eth_gasPrice — read from eosio.evm config table on-chain (cached).
        let gas_client = self.clone();
        module
            .register_async_method("eth_gasPrice", move |_params, _ctx, _ext| {
                let client = gas_client.clone();
                async move {
                    match client.get_gas_price().await {
                        Ok(price) => Ok::<U256, ErrorObject<'static>>(price),
                        Err(err) => {
                            warn!(target: "telos::gas", error = %err, "eth_gasPrice query failed");
                            Err(ErrorObject::owned(
                                -32603,
                                format!("Telos gas price unavailable: {err}"),
                                None::<()>,
                            ))
                        }
                    }
                }
            })
            .map_err(|e| {
                ErrorObject::owned(-32603, format!("register method: {e}"), None::<()>)
            })?;

        // eth_maxPriorityFeePerGas — Telos has no priority-fee market; mirror canonical RPC.
        module
            .register_async_method(
                "eth_maxPriorityFeePerGas",
                |_params, _ctx, _ext| async move {
                    Ok::<U256, ErrorObject<'static>>(U256::from(
                        TELOS_MAX_PRIORITY_FEE_PER_GAS_WEI,
                    ))
                },
            )
            .map_err(|e| {
                ErrorObject::owned(-32603, format!("register method: {e}"), None::<()>)
            })?;

        Ok(module)
    }

    /// Returns the current Telos gas price in wei, sourced from the on-chain
    /// `eosio.evm` config singleton table. Cached per the `gas_cache_seconds` arg.
    ///
    /// On a cache miss (first call, or TTL expired), POSTs `/v1/chain/get_table_rows`
    /// with `code=eosio.evm scope=eosio.evm table=config json=true limit=1`. The
    /// contract stores `gas_price` as a hex string (e.g. `"4c68cd444de"`) in wei.
    pub async fn get_gas_price(&self) -> Result<U256, antelope::AntelopeError> {
        // Fast path — return cached value if still fresh.
        if let Some((fetched_at, price)) = *self.inner.gas_price_cache.lock().unwrap() {
            if fetched_at.elapsed() < Duration::from_secs(self.inner.gas_cache_seconds as u64) {
                return Ok(price);
            }
        }

        // Cache miss — fetch from nodeos.
        let url = format!("{}/v1/chain/get_table_rows", self.inner.endpoint);
        let body = serde_json::json!({
            "code": "eosio.evm",
            "scope": "eosio.evm",
            "table": "config",
            "json": true,
            "limit": 1,
        });
        let resp = self.inner.http_client.post(&url).json(&body).send().await?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(antelope::AntelopeError::Nodeos {
                status: status.as_u16(),
                body: text,
            });
        }
        let parsed: GetTableRowsResponse = resp.json().await?;
        let row = parsed.rows.first().ok_or_else(|| antelope::AntelopeError::Nodeos {
            status: 200,
            body: "eosio.evm config table returned no rows".to_string(),
        })?;

        let price = parse_evm_gas_price(&row.gas_price).ok_or_else(|| {
            antelope::AntelopeError::Nodeos {
                status: 200,
                body: format!("malformed gas_price hex: {:?}", row.gas_price),
            }
        })?;

        // Update the cache. Multiple writers racing to insert the same value is fine.
        *self.inner.gas_price_cache.lock().unwrap() = Some((Instant::now(), price));

        debug!(target: "telos::gas", price = %price, "refreshed eosio.evm gas_price");
        Ok(price)
    }

    async fn get_info(&self) -> Result<GetInfoResponse, antelope::AntelopeError> {
        let url = format!("{}/v1/chain/get_info", self.inner.endpoint);
        let resp = self.inner.http_client.post(&url).send().await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(antelope::AntelopeError::Nodeos {
                status: status.as_u16(),
                body,
            });
        }
        let info: GetInfoResponse = resp.json().await?;
        Ok(info)
    }
}

/// Parses the `gas_price` field from an `eosio.evm` config row.
///
/// The field is a hex-encoded uint256 (with or without `0x` prefix), e.g.
/// `"4c68cd444de"` for 5,250,812,757,214 wei. Empty string and non-hex
/// inputs are treated as malformed and return None.
fn parse_evm_gas_price(raw: &str) -> Option<U256> {
    let trimmed = raw.trim_start_matches("0x");
    if trimmed.is_empty() {
        return None;
    }
    U256::from_str_radix(trimmed, 16).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_canonical_mainnet_gas_price() {
        // Live value observed on rpc.telos.net 2026-05-04 (`eth_gasPrice` = 0x4c68cd444de).
        let parsed = parse_evm_gas_price("4c68cd444de").expect("parses");
        assert_eq!(parsed, U256::from(5_250_812_757_214u64));
    }

    #[test]
    fn parses_zero_padded_nodeos_format() {
        // Actual format returned by `/v1/chain/get_table_rows` for eosio.evm.config:
        // a 64-char zero-padded hex string. Verified against mainnet.telos.net 2026-05-04.
        let raw = "000000000000000000000000000000000000000000000000000004c68cd444de";
        let parsed = parse_evm_gas_price(raw).expect("parses");
        assert_eq!(parsed, U256::from(5_250_812_757_214u64));
    }

    #[test]
    fn parses_with_0x_prefix() {
        let parsed = parse_evm_gas_price("0x4c68cd444de").expect("parses");
        assert_eq!(parsed, U256::from(5_250_812_757_214u64));
    }

    #[test]
    fn rejects_empty() {
        assert!(parse_evm_gas_price("").is_none());
        assert!(parse_evm_gas_price("0x").is_none());
    }

    #[test]
    fn rejects_non_hex() {
        assert!(parse_evm_gas_price("zzz").is_none());
    }
}
