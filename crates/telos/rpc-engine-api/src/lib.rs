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

/// Decode receipts from the CL extra fields.
/// The CL sends receipts as RLP-encoded bytes (legacy Receipt format from telos-reth v1).
/// Returns decoded receipts compatible with reth v2's Receipt type.
pub fn decode_receipts_from_extra_fields(
    raw_receipts: &[Vec<u8>],
) -> Vec<reth_ethereum_primitives::Receipt> {
    use alloy_rlp::Decodable;
    raw_receipts
        .iter()
        .filter_map(|raw| {
            match reth_ethereum_primitives::Receipt::decode(&mut raw.as_slice()) {
                Ok(receipt) => Some(receipt),
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        raw_len = raw.len(),
                        "Telos: failed to RLP-decode receipt from extra fields"
                    );
                    None
                }
            }
        })
        .collect()
}
