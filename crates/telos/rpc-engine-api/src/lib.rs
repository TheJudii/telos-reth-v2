//! Telos Engine API extension

/// Telos Engine API Structs
pub mod structs;

/// Telos Engine API State diff comparator
pub mod compare;

/// Parse extra fields JSON from a file path.
/// Returns Ok(None) if the file doesn't exist (expected for historical blocks).
/// Returns Ok(Some(fields)) on success.
/// Returns Err on read errors or parse errors.
pub fn parse_extra_fields_from_file(
    path: &str,
) -> Result<Option<structs::TelosEngineAPIExtraFields>, String> {
    match std::fs::read_to_string(path) {
        Ok(json_str) => {
            serde_json::from_str(&json_str)
                .map(Some)
                .map_err(|e| format!("JSON parse error: {e}"))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("File read error: {e}")),
    }
}

/// Convert a TelosExtraFieldReceipt (from CL JSON) into RLP-encoded bytes.
///
/// This builds an `EthereumReceipt` struct and RLP-encodes it. The payload validator
/// then decodes these bytes via the generic `<N::Receipt as Decodable>::decode()` path.
/// The round-trip works because `EthereumReceipt` derives both `RlpEncodable` and `RlpDecodable`.
pub fn telos_receipt_to_rlp_bytes(
    telos_receipt: &structs::TelosExtraFieldReceipt,
) -> Vec<u8> {
    use alloy_consensus::TxType;
    use alloy_rlp::Encodable;

    let tx_type = match telos_receipt.tx_type.as_str() {
        "Legacy" => TxType::Legacy,
        "Eip2930" => TxType::Eip2930,
        "Eip1559" => TxType::Eip1559,
        "Eip4844" => TxType::Eip4844,
        "Eip7702" => TxType::Eip7702,
        other => {
            tracing::warn!(
                tx_type = other,
                "Telos: unknown tx_type in CL receipt, defaulting to Legacy"
            );
            TxType::Legacy
        }
    };

    let receipt = reth_ethereum_primitives::Receipt {
        tx_type,
        success: telos_receipt.success,
        cumulative_gas_used: telos_receipt.cumulative_gas_used,
        logs: telos_receipt.logs.clone(),
    };

    let mut buf = Vec::new();
    receipt.encode(&mut buf);
    buf
}

/// Convert a batch of CL receipts to RLP-encoded byte arrays.
pub fn telos_receipts_to_rlp(
    telos_receipts: &[structs::TelosExtraFieldReceipt],
) -> Vec<Vec<u8>> {
    telos_receipts.iter().map(telos_receipt_to_rlp_bytes).collect()
}
