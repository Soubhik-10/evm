//! EIP-8141 expiry verifier fork-state transition.

use crate::{
    block::{BlockExecutionError, StateDB},
    Evm,
};
use alloy_eips::eip8141::{EXPIRY_VERIFIER, EXPIRY_VERIFIER_RUNTIME};
use alloy_primitives::{keccak256, Bytes};
use revm::{
    state::{Account, Bytecode, TransactionId},
    Database, DatabaseCommit,
};

/// Installs the canonical expiry verifier runtime code.
///
/// This is a fork-state transition, not a system transaction. It preserves the account's nonce,
/// balance, and storage, and has no receipt or gas cost.
pub(crate) fn install_expiry_verifier(
    evm: &mut impl Evm<DB: StateDB>,
) -> Result<(), BlockExecutionError> {
    let db = evm.db_mut();
    let runtime_hash = keccak256(EXPIRY_VERIFIER_RUNTIME);
    let current = db.basic(EXPIRY_VERIFIER).map_err(BlockExecutionError::other)?;
    if current.as_ref().is_some_and(|account| account.code_hash == runtime_hash) {
        return Ok(());
    }

    let mut account = current
        .map(Account::from)
        .unwrap_or_else(|| Account::new_not_existing(TransactionId::ZERO));
    account.info.code_hash = runtime_hash;
    account.info.code =
        Some(Bytecode::new_legacy(Bytes::copy_from_slice(&EXPIRY_VERIFIER_RUNTIME)));
    account.mark_touch();
    db.commit([(EXPIRY_VERIFIER, account)].into_iter().collect());
    Ok(())
}
