//! Offline byte-for-byte conformance binary: emits packed_trx_hex for the same
//! fixed inputs as conformance_pyntelope.py. Diffing the two must yield 0.
//!
//! This file is dropped in as `crates/telos/rpc/src/bin/antelope_conformance.rs`
//! and built with `cargo run -p reth-telos-rpc --bin antelope_conformance`.
//!
//! The module under test is `reth_telos_rpc::antelope`.

use alloy_primitives::B256;
use reth_telos_rpc::antelope::{
    name_to_u64, ref_block_num, ref_block_prefix, serialize_raw_action_data,
    PackedAction, PackedTransaction,
};

fn main() {
    // --- Fixed inputs (must match conformance_pyntelope.py) ---
    const CHAIN_ID_HEX: &str =
        "1eaa0824707c8c16bd25145493bf062aecddfeb56c736f6ba6397a3c4d040c75";
    const EXPIRATION_UNIX: u32 = 1_700_000_000;
    const REF_BLOCK_NUM: u16 = 0x1234;
    const REF_BLOCK_PREFIX: u32 = 0xabcdef01;
    const TX_HEX: &str = "deadbeef";

    let tx_bytes: Vec<u8> = hex::decode(TX_HEX).unwrap();

    let ram_payer = name_to_u64("eosio.evm").unwrap();
    let contract = name_to_u64("eosio.evm").unwrap();
    let action_name = name_to_u64("raw").unwrap();
    let signer_actor = name_to_u64("rpc.evm").unwrap();
    let signer_permission = name_to_u64("rpc").unwrap();

    let action_data = serialize_raw_action_data(ram_payer, &tx_bytes, false, None);
    let action = PackedAction {
        account: contract,
        name: action_name,
        authorization: vec![(signer_actor, signer_permission)],
        data: action_data.clone(),
    };

    let packed = PackedTransaction {
        expiration: EXPIRATION_UNIX,
        ref_block_num: REF_BLOCK_NUM,
        ref_block_prefix: REF_BLOCK_PREFIX,
        max_net_usage_words: 0,
        max_cpu_usage_ms: 0,
        delay_sec: 0,
        actions: vec![action],
    };
    let packed_bytes = packed.serialize();

    println!("=== rust reference output ===");
    println!("CHAIN_ID        = {}", CHAIN_ID_HEX);
    println!("EXPIRATION_UNIX = {}", EXPIRATION_UNIX);
    println!("REF_BLOCK_NUM   = 0x{:04x}", REF_BLOCK_NUM);
    println!("REF_BLOCK_PREFIX= 0x{:08x}", REF_BLOCK_PREFIX);
    println!("TX_HEX          = {}", TX_HEX);
    println!();
    println!("packed_trx_hex  = {}", hex::encode(&packed_bytes));
    println!("packed_trx_len  = {}", packed_bytes.len());
    println!();
    println!("action_data_hex = {}", hex::encode(&action_data));
    println!("action_data_len = {}", action_data.len());

    // Silence unused-import warning for B256 / ref_block_{num,prefix} if the
    // helpers aren't exercised here; keep them exported so consumers see them.
    let _ = B256::ZERO;
    let _ = ref_block_num(0);
    let _ = ref_block_prefix(&B256::ZERO);
}
