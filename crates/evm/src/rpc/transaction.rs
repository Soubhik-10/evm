use crate::{
    env::BlockEnvironment,
    rpc::{CallFees, CallFeesError},
    EvmEnv, FromRecoveredTx,
};
use alloy_consensus::{TxEip8141, TxType};
use alloy_eips::eip8141::TransactionFees;
use alloy_primitives::{TxKind, U256};
use alloy_rpc_types_eth::request::{TransactionInputError, TransactionRequest};
use core::fmt::Debug;
use revm::{context::TxEnv, context_interface::either::Either};
use thiserror::Error;

/// Converts `self` into `T`.
///
/// Should create an executable transaction environment using [`TransactionRequest`].
pub trait TryIntoTxEnv<
    T,
    Spec = revm::primitives::hardfork::SpecId,
    BlockEnv = revm::context::BlockEnv,
>
{
    /// An associated error that can occur during the conversion.
    type Err;

    /// Performs the conversion.
    fn try_into_tx_env(self, evm_env: &EvmEnv<Spec, BlockEnv>) -> Result<T, Self::Err>;
}

/// An Ethereum specific transaction environment error than can occur during conversion from
/// [`TransactionRequest`].
#[derive(Debug, Error)]
pub enum EthTxEnvError {
    /// Error while decoding or validating transaction request fees.
    #[error(transparent)]
    CallFees(#[from] CallFeesError),
    /// Both data and input fields are set and not equal.
    #[error(transparent)]
    Input(#[from] TransactionInputError),
    /// An EIP-8141 request included fields which are not part of the frame
    /// transaction envelope and therefore cannot be silently normalized away.
    #[error("EIP-8141 transaction request contains non-canonical outer fields")]
    Eip8141InvalidOuterFields,
}

impl<Spec, Block: BlockEnvironment> TryIntoTxEnv<TxEnv, Spec, Block> for TransactionRequest {
    type Err = EthTxEnvError;

    fn try_into_tx_env(self, evm_env: &EvmEnv<Spec, Block>) -> Result<TxEnv, Self::Err> {
        // Ensure that if versioned hashes are set, they're not empty
        if self.blob_versioned_hashes.as_ref().is_some_and(|hashes| hashes.is_empty()) {
            return Err(CallFeesError::BlobTransactionMissingBlobHashes.into());
        }

        let tx_type = self.minimal_tx_type() as u8;

        let Self {
            from,
            to,
            gas_price,
            max_fee_per_gas,
            max_priority_fee_per_gas,
            gas,
            value,
            input,
            nonce,
            access_list,
            chain_id,
            blob_versioned_hashes,
            max_fee_per_blob_gas,
            authorization_list,
            frames,
            signatures,
            transaction_type: _,
            sidecar: _,
            ..
        } = self;

        let input = input.try_into_unique_input().map_err(EthTxEnvError::from)?.unwrap_or_default();

        if tx_type == TxType::Eip8141 as u8
            && (to.is_some()
                || gas_price.is_some()
                || gas.is_some()
                || value.is_some_and(|value| !value.is_zero())
                || !input.is_empty()
                || access_list.is_some()
                || authorization_list.is_some())
        {
            return Err(EthTxEnvError::Eip8141InvalidOuterFields);
        }

        let requested_max_fee_per_gas = max_fee_per_gas;
        let requested_max_priority_fee_per_gas = max_priority_fee_per_gas;
        let requested_max_fee_per_blob_gas = max_fee_per_blob_gas;

        let CallFees { max_priority_fee_per_gas, gas_price, max_fee_per_blob_gas } =
            CallFees::ensure_fees(
                gas_price.map(U256::from),
                max_fee_per_gas.map(U256::from),
                max_priority_fee_per_gas.map(U256::from),
                U256::from(evm_env.block_env().basefee()),
                blob_versioned_hashes.as_deref(),
                max_fee_per_blob_gas.map(U256::from),
                evm_env.block_env().blob_gasprice().map(U256::from),
            )?;

        let gas_limit = gas.unwrap_or(
            // Use maximum allowed gas limit. The reason for this
            // is that both Erigon and Geth use pre-configured gas cap even if
            // it's possible to derive the gas limit from the block:
            // <https://github.com/ledgerwatch/erigon/blob/eae2d9a79cb70dbe30b3a6b79c436872e4605458/cmd/rpcdaemon/commands/trace_adhoc.go#L956
            // https://github.com/ledgerwatch/erigon/blob/eae2d9a79cb70dbe30b3a6b79c436872e4605458/eth/ethconfig/config.go#L94>
            evm_env.block_env().gas_limit(),
        );

        let chain_id = chain_id.unwrap_or(evm_env.cfg_env().chain_id);

        let caller = from.unwrap_or_default();

        let nonce = nonce.unwrap_or_default();

        if tx_type == TxType::Eip8141 as u8 {
            let tx = TxEip8141 {
                chain_id,
                nonce,
                sender: caller,
                frames: frames.unwrap_or_default(),
                signatures: signatures.unwrap_or_default(),
                fees: TransactionFees {
                    max_priority_fee_per_gas: requested_max_priority_fee_per_gas
                        .map(U256::from)
                        .unwrap_or_default(),
                    max_fee_per_gas: requested_max_fee_per_gas.map(U256::from).unwrap_or(gas_price),
                    max_fee_per_blob_gas: requested_max_fee_per_blob_gas
                        .map(U256::from)
                        .unwrap_or_default(),
                },
                blob_versioned_hashes: blob_versioned_hashes.unwrap_or_default(),
            };
            return Ok(TxEnv::from_recovered_tx(&tx, caller));
        }

        let env = TxEnv {
            tx_type,
            gas_limit,
            nonce,
            caller,
            gas_price: gas_price.saturating_to(),
            gas_priority_fee: max_priority_fee_per_gas.map(|v| v.saturating_to()),
            kind: to.unwrap_or(TxKind::Create),
            value: value.unwrap_or_default(),
            data: input,
            chain_id: Some(chain_id),
            access_list: access_list.unwrap_or_default(),
            // EIP-4844 fields
            blob_hashes: blob_versioned_hashes.unwrap_or_default(),
            max_fee_per_blob_gas: max_fee_per_blob_gas
                .map(|v| v.saturating_to())
                .unwrap_or_default(),
            // EIP-7702 fields
            authorization_list: authorization_list
                .unwrap_or_default()
                .into_iter()
                .map(Either::Left)
                .collect(),
            frame_transaction: None,
        };

        Ok(env)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::{Address, TxKind};

    #[test]
    fn frame_request_rejects_outer_transaction_fields() {
        let request = TransactionRequest {
            to: Some(TxKind::Call(Address::ZERO)),
            frames: Some(Vec::new()),
            ..Default::default()
        };
        let evm_env: EvmEnv = EvmEnv::default();

        assert!(matches!(
            request.try_into_tx_env(&evm_env),
            Err(EthTxEnvError::Eip8141InvalidOuterFields)
        ));
    }
}
