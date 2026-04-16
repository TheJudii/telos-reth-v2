use alloy_primitives::{Address, Bytes, Log, U256};
use serde::{Deserialize, Serialize};

/// Telos EVM Account Table Row
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TelosAccountTableRow {
    /// Removed - if true, this row was removed from storage
    pub removed: bool,
    /// Address
    pub address: Address,
    /// Account
    pub account: String,
    /// Nonce
    pub nonce: u64,
    /// Code
    pub code: Bytes,
    /// Balance
    pub balance: U256,
}

/// Telos EVM Account State Table Row
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TelosAccountStateTableRow {
    /// Removed - if true, this row was removed from storage
    pub removed: bool,
    /// Address
    pub address: Address,
    /// Key
    pub key: U256,
    /// Value
    pub value: U256,
}

/// Receipt format as written by telos-consensus-client.
/// Matches the CL JSON: {"tx_type": "Legacy", "success": true, "cumulative_gas_used": 21000, "logs": []}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TelosExtraFieldReceipt {
    /// Transaction type as string ("Legacy", "Eip2930", "Eip1559", "Eip4844", "Eip7702")
    pub tx_type: String,
    /// Whether the transaction executed successfully
    pub success: bool,
    /// Cumulative gas used up to and including this transaction
    pub cumulative_gas_used: u64,
    /// Logs emitted by this transaction
    pub logs: Vec<Log>,
}

/// Telos Engine API Extra Fields
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TelosEngineAPIExtraFields {
    /// State Diffs for Account Table
    pub statediffs_account: Option<Vec<TelosAccountTableRow>>,
    /// State Diffs for Account State Table
    pub statediffs_accountstate: Option<Vec<TelosAccountStateTableRow>>,
    /// Revision changes in block
    pub revision_changes: Option<(u64, u64)>,
    /// Gas price changes in block
    pub gasprice_changes: Option<(u64, U256)>,
    /// New addresses using `create` action in block
    pub new_addresses_using_create: Option<Vec<(u64, U256)>>,
    /// New addresses using `openwallet` action in block
    pub new_addresses_using_openwallet: Option<Vec<(u64, U256)>>,
    /// Receipts produced by telos.evm contract (structured, from CL)
    pub receipts: Option<Vec<TelosExtraFieldReceipt>>,
}
