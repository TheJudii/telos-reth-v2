//! Telos EVM configuration.
//!
//! Telos mainnet's legacy genesis file does not carry every Ethereum fork activation used by
//! the live EVM contract. Changing that genesis file changes the genesis hash and makes existing
//! datadirs unusable, so Telos keeps the chainspec unchanged and only lifts revm execution to
//! the minimum fork the live chain requires.

use alloy_consensus::Header;
use alloy_eips::Decodable2718;
use alloy_evm::{
    eth::{EthBlockExecutionCtx, EthBlockExecutorFactory},
    EthEvmFactory,
};
use alloy_primitives::{Bytes, B256, U256};
use alloy_rpc_types_engine::ExecutionData;
use reth_chainspec::{ChainSpec, EthChainSpec, EthereumHardforks, Hardforks};
use reth_ethereum_primitives::EthPrimitives;
use reth_evm::{
    eth::spec::EthExecutorSpec, ConfigureEngineEvm, ConfigureEvm, EvmEnv, EvmEnvFor,
    ExecutableTxIterator, ExecutionCtxFor, NextBlockEnvAttributes,
};
use reth_evm_ethereum::{EthBlockAssembler, RethReceiptBuilder};
use reth_node_api::{FullNodeTypes, NodeTypes};
use reth_node_builder::{components::ExecutorBuilder, BuilderContext};
use reth_node_ethereum::EthEvmConfig;
use reth_primitives_traits::{SealedBlock, SealedHeader, SignedTransaction, TxTy};
use reth_storage_errors::any::AnyError;
use revm::primitives::hardfork::SpecId;
use std::{convert::Infallible, sync::Arc};

/// Lowest revm spec Telos mainnet needs for RPC simulation and payload execution.
///
/// This enables EIP-3855 (`PUSH0`) for modern Solidity bytecode while avoiding a chainspec
/// mutation that would change the genesis hash.
const TELOS_MIN_REVM_SPEC: SpecId = SpecId::SHANGHAI;

fn raise_to_telos_min_spec(mut env: EvmEnv<SpecId>) -> EvmEnv<SpecId> {
    if env.cfg_env.spec < TELOS_MIN_REVM_SPEC {
        env.cfg_env.spec = TELOS_MIN_REVM_SPEC;
    }
    if env.cfg_env.spec >= SpecId::MERGE {
        env.block_env.difficulty = U256::ZERO;
        env.block_env.prevrandao.get_or_insert(B256::ZERO);
    }
    env
}

/// Telos wrapper around the stock Ethereum EVM config.
#[derive(Debug, Clone)]
pub struct TelosEvmConfig<C = ChainSpec> {
    inner: EthEvmConfig<C>,
}

impl<ChainSpec> TelosEvmConfig<ChainSpec> {
    /// Creates a new Telos EVM configuration with the given chain spec.
    pub fn new(chain_spec: Arc<ChainSpec>) -> Self {
        Self { inner: EthEvmConfig::new(chain_spec) }
    }
}

impl<ChainSpec> TelosEvmConfig<ChainSpec> {
    /// Returns the underlying chain spec.
    pub const fn chain_spec(&self) -> &Arc<ChainSpec> {
        self.inner.chain_spec()
    }
}

impl<ChainSpec> ConfigureEvm for TelosEvmConfig<ChainSpec>
where
    EthEvmConfig<ChainSpec>: ConfigureEvm<
        Primitives = EthPrimitives,
        Error = Infallible,
        NextBlockEnvCtx = NextBlockEnvAttributes,
        BlockExecutorFactory = EthBlockExecutorFactory<
            RethReceiptBuilder,
            Arc<ChainSpec>,
            EthEvmFactory,
        >,
        BlockAssembler = EthBlockAssembler<ChainSpec>,
    >,
    ChainSpec: EthExecutorSpec + EthChainSpec<Header = Header> + Hardforks + 'static,
{
    type Primitives = EthPrimitives;
    type Error = Infallible;
    type NextBlockEnvCtx = NextBlockEnvAttributes;
    type BlockExecutorFactory =
        EthBlockExecutorFactory<RethReceiptBuilder, Arc<ChainSpec>, EthEvmFactory>;
    type BlockAssembler = EthBlockAssembler<ChainSpec>;

    fn block_executor_factory(&self) -> &Self::BlockExecutorFactory {
        self.inner.block_executor_factory()
    }

    fn block_assembler(&self) -> &Self::BlockAssembler {
        self.inner.block_assembler()
    }

    fn evm_env(&self, header: &Header) -> Result<EvmEnvFor<Self>, Self::Error> {
        self.inner.evm_env(header).map(raise_to_telos_min_spec)
    }

    fn next_evm_env(
        &self,
        parent: &Header,
        attributes: &NextBlockEnvAttributes,
    ) -> Result<EvmEnvFor<Self>, Self::Error> {
        self.inner.next_evm_env(parent, attributes).map(raise_to_telos_min_spec)
    }

    fn context_for_block<'a>(
        &self,
        block: &'a SealedBlock<reth_ethereum_primitives::Block>,
    ) -> Result<EthBlockExecutionCtx<'a>, Self::Error> {
        self.inner.context_for_block(block)
    }

    fn context_for_next_block(
        &self,
        parent: &SealedHeader<Header>,
        attributes: Self::NextBlockEnvCtx,
    ) -> Result<EthBlockExecutionCtx<'_>, Self::Error> {
        self.inner.context_for_next_block(parent, attributes)
    }
}

impl<ChainSpec> ConfigureEngineEvm<ExecutionData> for TelosEvmConfig<ChainSpec>
where
    EthEvmConfig<ChainSpec>: ConfigureEngineEvm<ExecutionData>
        + ConfigureEvm<
            Primitives = EthPrimitives,
            Error = Infallible,
            NextBlockEnvCtx = NextBlockEnvAttributes,
            BlockExecutorFactory = EthBlockExecutorFactory<
                RethReceiptBuilder,
                Arc<ChainSpec>,
                EthEvmFactory,
            >,
            BlockAssembler = EthBlockAssembler<ChainSpec>,
        >,
    ChainSpec: EthExecutorSpec + EthChainSpec<Header = Header> + Hardforks + 'static,
{
    fn evm_env_for_payload(&self, payload: &ExecutionData) -> Result<EvmEnvFor<Self>, Self::Error> {
        self.inner.evm_env_for_payload(payload).map(raise_to_telos_min_spec)
    }

    fn context_for_payload<'a>(
        &self,
        payload: &'a ExecutionData,
    ) -> Result<ExecutionCtxFor<'a, Self>, Self::Error> {
        self.inner.context_for_payload(payload)
    }

    fn tx_iterator_for_payload(
        &self,
        payload: &ExecutionData,
    ) -> Result<impl ExecutableTxIterator<Self>, Self::Error> {
        let txs = payload.payload.transactions().clone();
        let convert = |tx: Bytes| {
            let tx =
                TxTy::<Self::Primitives>::decode_2718_exact(tx.as_ref()).map_err(AnyError::new)?;
            let signer = tx.try_recover().map_err(AnyError::new)?;
            Ok::<_, AnyError>(tx.with_signer(signer))
        };

        Ok((txs, convert))
    }
}

/// Telos executor/EVM builder.
#[derive(Debug, Default, Clone, Copy)]
#[non_exhaustive]
pub struct TelosExecutorBuilder;

impl<Types, Node> ExecutorBuilder<Node> for TelosExecutorBuilder
where
    Types: NodeTypes<
        ChainSpec: Hardforks + EthExecutorSpec + EthereumHardforks,
        Primitives = EthPrimitives,
    >,
    Node: FullNodeTypes<Types = Types>,
{
    type EVM = TelosEvmConfig<Types::ChainSpec>;

    async fn build_evm(self, ctx: &BuilderContext<Node>) -> eyre::Result<Self::EVM> {
        Ok(TelosEvmConfig::new(ctx.chain_spec()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_genesis::Genesis;
    use reth_chainspec::{Chain, ChainSpecBuilder, EthereumHardfork, ForkCondition};

    #[test]
    fn legacy_telos_chainspec_uses_shanghai_revm_without_mutating_hardforks() {
        let chain_spec = Arc::new(
            ChainSpecBuilder::default()
                .chain(Chain::from_id(40))
                .genesis(Genesis::default())
                .with_fork(EthereumHardfork::Frontier, ForkCondition::Block(0))
                .with_fork(EthereumHardfork::Berlin, ForkCondition::Block(0))
                .build(),
        );

        assert!(!chain_spec.is_shanghai_active_at_timestamp(0));

        let env = TelosEvmConfig::new(chain_spec.clone()).evm_env(&Header::default()).unwrap();

        assert_eq!(env.cfg_env.chain_id, 40);
        assert_eq!(env.cfg_env.spec, SpecId::SHANGHAI);
        assert_eq!(env.block_env.difficulty, U256::ZERO);
        assert_eq!(env.block_env.prevrandao, Some(B256::ZERO));
        assert!(!chain_spec.is_shanghai_active_at_timestamp(0));
    }

    #[test]
    fn telos_evm_does_not_downgrade_newer_specs() {
        let chain_spec = Arc::new(
            ChainSpecBuilder::default()
                .chain(Chain::from_id(40))
                .genesis(Genesis::default())
                .cancun_activated()
                .build(),
        );

        let env = TelosEvmConfig::new(chain_spec).evm_env(&Header::default()).unwrap();

        assert!(env.cfg_env.spec >= SpecId::CANCUN);
    }
}
