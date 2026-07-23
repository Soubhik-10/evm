//! Abstraction over receipt building logic to allow plugging different primitive types into
//! [`super::EthBlockExecutor`].

use crate::Evm;
use alloy_consensus::{Eip658Value, ReceiptEnvelope, TransactionEnvelope, TxEnvelope, TxType};
use alloy_eips::eip8141::FrameReceiptPayload;
use revm::{context::result::ExecutionResult, state::EvmState};

/// Context for building a receipt.
#[derive(Debug)]
pub struct ReceiptBuilderCtx<'a, T, E: Evm> {
    /// Transaction
    pub tx_type: T,
    /// Reference to EVM. State changes should not be committed to inner database when building
    /// receipt so that [`ReceiptBuilder`] can use data from state before transaction execution.
    pub evm: &'a E,
    /// Result of transaction execution.
    pub result: ExecutionResult<E::HaltReason>,
    /// Reference to EVM state after execution.
    pub state: &'a EvmState,
    /// Cumulative gas used.
    pub cumulative_gas_used: u64,
}

/// Type that knows how to build a receipt based on execution result.
#[auto_impl::auto_impl(&, Arc)]
pub trait ReceiptBuilder {
    /// Transaction type.
    type Transaction: TransactionEnvelope;
    /// Receipt type.
    type Receipt;

    /// Builds a receipt given a transaction and the result of the execution.
    fn build_receipt<E: Evm>(
        &self,
        ctx: ReceiptBuilderCtx<'_, <Self::Transaction as TransactionEnvelope>::TxType, E>,
    ) -> Self::Receipt;
}

/// Receipt builder operating on Alloy types.
#[derive(Debug, Default, Clone, Copy)]
#[non_exhaustive]
pub struct AlloyReceiptBuilder;

impl ReceiptBuilder for AlloyReceiptBuilder {
    type Transaction = TxEnvelope;
    type Receipt = ReceiptEnvelope;

    fn build_receipt<E: Evm>(&self, ctx: ReceiptBuilderCtx<'_, TxType, E>) -> Self::Receipt {
        build_alloy_receipt(ctx.tx_type, ctx.result, ctx.cumulative_gas_used)
    }
}

fn build_alloy_receipt<Halt>(
    tx_type: TxType,
    result: ExecutionResult<Halt>,
    cumulative_gas_used: u64,
) -> ReceiptEnvelope {
    if tx_type == TxType::Eip8141 {
        let ExecutionResult::FrameTransaction { payer, frame_receipts, .. } = result else {
            panic!("EIP-8141 execution must return a frame transaction result")
        };
        return ReceiptEnvelope::Eip8141(FrameReceiptPayload {
            cumulative_gas_used,
            payer,
            frame_receipts,
        });
    }

    assert!(
        !matches!(result, ExecutionResult::FrameTransaction { .. }),
        "frame transaction result requires an EIP-8141 receipt"
    );
    let receipt = alloy_consensus::Receipt {
        status: Eip658Value::Eip658(result.is_success()),
        cumulative_gas_used,
        logs: result.into_logs(),
    }
    .with_bloom();

    match tx_type {
        TxType::Legacy => ReceiptEnvelope::Legacy(receipt),
        TxType::Eip2930 => ReceiptEnvelope::Eip2930(receipt),
        TxType::Eip1559 => ReceiptEnvelope::Eip1559(receipt),
        TxType::Eip4844 => ReceiptEnvelope::Eip4844(receipt),
        TxType::Eip7702 => ReceiptEnvelope::Eip7702(receipt),
        TxType::Eip8141 => unreachable!("handled above"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_eips::eip8141::{FrameReceipt, FrameStatus};
    use alloy_primitives::{address, Log};
    use revm::context::result::{HaltReason, ResultGas};

    #[test]
    fn builds_frame_receipt_payload() {
        let payer = address!("0000000000000000000000000000000000000001");
        let frame_receipts = vec![FrameReceipt {
            status: FrameStatus::Success,
            gas_used: 42,
            logs: vec![Log::default()],
        }];
        let result = ExecutionResult::<HaltReason>::FrameTransaction {
            gas: ResultGas::default().with_total_gas_spent(42),
            payer,
            logs: frame_receipts[0].logs.clone(),
            frame_receipts: frame_receipts.clone(),
        };

        let receipt = build_alloy_receipt(TxType::Eip8141, result, 100);
        let payload = receipt.as_eip8141().expect("frame receipt");
        assert_eq!(payload.cumulative_gas_used, 100);
        assert_eq!(payload.payer, payer);
        assert_eq!(payload.frame_receipts, frame_receipts);
    }
}
