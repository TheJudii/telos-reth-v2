//! Telos engine validator - overrides block hash validation for legacy consensus client
//! compatibility.
//!
//! The Telos consensus client (alloy 0.3.x) computes block_hash with base_fee_per_gas=None
//! but sends a non-zero base_fee_per_gas in the ExecutionPayloadV1. Reth v1.11.x recomputes
//! the hash using the payload's base_fee_per_gas and gets a mismatch.
//!
//! Fix: trust the block_hash provided by the consensus client, skip hash recomputation.

use alloy_rpc_types_engine::{ExecutionData, PayloadError};
use reth_chainspec::{EthChainSpec, EthereumHardforks};
use reth_engine_primitives::{EngineApiValidator, PayloadValidator};
use reth_ethereum_engine_primitives::EthPayloadAttributes;
use reth_ethereum_primitives::Block;
use reth_node_api::PayloadTypes;
use reth_payload_primitives::{
    validate_execution_requests, validate_version_specific_fields, EngineApiMessageVersion,
    EngineObjectValidationError, NewPayloadError, PayloadOrAttributes,
};
use reth_payload_validator::{cancun, prague, shanghai};
use reth_primitives_traits::{RecoveredBlock, SealedBlock, SignedTransaction, SignerRecoverable};
use std::sync::Arc;

/// Telos engine validator that trusts block_hash from the consensus client.
#[derive(Debug, Clone)]
pub struct TelosEngineValidator<ChainSpec = reth_chainspec::ChainSpec> {
    chain_spec: Arc<ChainSpec>,
}

impl<ChainSpec> TelosEngineValidator<ChainSpec> {
    /// Create a new Telos engine validator.
    pub const fn new(chain_spec: Arc<ChainSpec>) -> Self {
        Self { chain_spec }
    }
}

/// Convert payload to block, trusting the block_hash from the consensus client.
fn telos_ensure_well_formed_payload(
    chain_spec: &impl EthereumHardforks,
    payload: ExecutionData,
) -> Result<SealedBlock<Block>, PayloadError> {
    let ExecutionData { payload, sidecar } = payload;

    // KEY FIX: preserve the hash from the consensus client, don't recompute it
    let trusted_hash = payload.block_hash();

    // Parse the block (this sets base_fee_per_gas=Some(...) in the header)
    let block: Block = payload.try_into_block_with_sidecar(&sidecar)?;

    // The legacy Telos chain stores post-transition headers without base_fee_per_gas in the
    // canonical header/RPC representation, even though the execution payload carries a non-zero
    // base fee. Preserve that legacy representation here before persisting the block so RPC block
    // queries and downstream hash lineage stay compatible with existing Telos infra.
    let alloy_consensus::Block { mut header, body } = block;
    header.base_fee_per_gas = None;

    // Seal with the trusted hash instead of recomputing via seal_slow()
    let sealed_block = SealedBlock::<Block>::from_parts_unchecked(header, body, trusted_hash);

    // Still validate EIP structural requirements (just not the hash)
    shanghai::ensure_well_formed_fields(
        sealed_block.body(),
        chain_spec.is_shanghai_active_at_timestamp(sealed_block.timestamp),
    )?;

    cancun::ensure_well_formed_fields(
        &sealed_block,
        sidecar.cancun(),
        chain_spec.is_cancun_active_at_timestamp(sealed_block.timestamp),
    )?;

    prague::ensure_well_formed_fields(
        sealed_block.body(),
        sidecar.prague(),
        chain_spec.is_prague_active_at_timestamp(sealed_block.timestamp),
    )?;

    Ok(sealed_block)
}

impl<ChainSpec, Types> PayloadValidator<Types> for TelosEngineValidator<ChainSpec>
where
    ChainSpec: EthChainSpec + EthereumHardforks + 'static,
    Types: PayloadTypes<ExecutionData = ExecutionData>,
{
    type Block = Block;

    fn convert_payload_to_block(
        &self,
        payload: ExecutionData,
    ) -> Result<SealedBlock<Self::Block>, NewPayloadError> {
        telos_ensure_well_formed_payload(self.chain_spec.as_ref(), payload).map_err(Into::into)
    }

    fn ensure_well_formed_payload(
        &self,
        payload: ExecutionData,
    ) -> Result<RecoveredBlock<Self::Block>, NewPayloadError> {
        let sealed_block =
            <TelosEngineValidator<ChainSpec> as PayloadValidator<Types>>::convert_payload_to_block(
                self, payload,
            )?;

        // Telos: fallback for system transactions with non-standard signatures
        // Use Address::ZERO for any tx that fails ECDSA recovery
        let hash = sealed_block.hash();
        let (sealed_header, body) = sealed_block.split_sealed_header_body();
        let mut senders: Vec<alloy_primitives::Address> =
            Vec::with_capacity(body.transactions.len());
        for tx in &body.transactions {
            let sender: alloy_primitives::Address =
                tx.recover_signer().unwrap_or(alloy_primitives::Address::ZERO);
            senders.push(sender);
        }
        let header = sealed_header.unseal();
        let block = reth_ethereum_primitives::Block { header, body };
        Ok(RecoveredBlock::new(block, senders, hash))
    }
}

impl<ChainSpec, Types> EngineApiValidator<Types> for TelosEngineValidator<ChainSpec>
where
    ChainSpec: EthChainSpec + EthereumHardforks + 'static,
    Types: PayloadTypes<PayloadAttributes = EthPayloadAttributes, ExecutionData = ExecutionData>,
{
    fn validate_version_specific_fields(
        &self,
        version: EngineApiMessageVersion,
        payload_or_attrs: PayloadOrAttributes<'_, Types::ExecutionData, EthPayloadAttributes>,
    ) -> Result<(), EngineObjectValidationError> {
        payload_or_attrs
            .execution_requests()
            .map(|requests| validate_execution_requests(requests))
            .transpose()?;

        validate_version_specific_fields(&self.chain_spec, version, payload_or_attrs)
    }

    fn ensure_well_formed_attributes(
        &self,
        version: EngineApiMessageVersion,
        attributes: &EthPayloadAttributes,
    ) -> Result<(), EngineObjectValidationError> {
        validate_version_specific_fields::<ExecutionData, EthPayloadAttributes, _>(
            &self.chain_spec,
            version,
            PayloadOrAttributes::from(attributes),
        )
    }
}

// ---------------------------------------------------------------------------
// Builder wiring: TelosEngineValidatorBuilder
// ---------------------------------------------------------------------------
//
// This is the wiring layer that lets the Telos node's add-ons use
// `TelosEngineValidator` above instead of the stock Ethereum validator.
// It mirrors `EthereumEngineValidatorBuilder` from reth_node_ethereum, but
// constructs a `TelosEngineValidator` in `build()`.

use reth_node_api::NodeTypes;
use reth_node_builder::{rpc::PayloadValidatorBuilder, FullNodeComponents};

/// Builder for the Telos engine validator.
#[derive(Debug, Clone, Default)]
pub struct TelosEngineValidatorBuilder;

impl<Node, Types> PayloadValidatorBuilder<Node> for TelosEngineValidatorBuilder
where
    Types: NodeTypes<
        ChainSpec: reth_chainspec::Hardforks + reth_chainspec::EthereumHardforks + Clone + 'static,
        Payload: reth_engine_primitives::EngineTypes<ExecutionData = ExecutionData>
                     + reth_payload_primitives::PayloadTypes<
            PayloadAttributes = reth_ethereum_engine_primitives::EthPayloadAttributes,
        >,
        Primitives = reth_ethereum_primitives::EthPrimitives,
    >,
    Node: FullNodeComponents<Types = Types>,
{
    type Validator = TelosEngineValidator<Types::ChainSpec>;

    async fn build(
        self,
        ctx: &reth_node_api::AddOnsContext<'_, Node>,
    ) -> eyre::Result<Self::Validator> {
        Ok(TelosEngineValidator::new(ctx.config.chain.clone()))
    }
}
