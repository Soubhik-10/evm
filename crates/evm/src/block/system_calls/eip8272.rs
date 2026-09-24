//! EIP-8272 recent-root contract fork-state transition.

use crate::{
    block::{BlockExecutionError, BlockValidationError, StateDB},
    Evm,
};
use alloy_eips::eip8272::{RECENT_ROOT_ADDRESS, RECENT_ROOT_CODE, RECENT_ROOT_CODE_HASH};
use revm::{
    state::{Account, Bytecode, TransactionId},
    Database, DatabaseCommit,
};

/// Installs the EIP-8272 recent-root runtime at its reserved address.
///
/// This is a fork-state transition, not a system transaction. A missing account is created with
/// nonce one; an existing empty-code account retains its balance and has its nonce raised to one.
/// Once installed, a different non-empty runtime is a consensus-invalid state transition rather
/// than something that may be silently repaired by a later block.
pub(crate) fn install_recent_root_contract(
    evm: &mut impl Evm<DB: StateDB>,
) -> Result<(), BlockExecutionError> {
    let db = evm.db_mut();
    let current = db.basic(RECENT_ROOT_ADDRESS).map_err(BlockExecutionError::other)?;
    if current.as_ref().is_some_and(|account| account.code_hash == RECENT_ROOT_CODE_HASH) {
        return Ok(());
    }
    if current.as_ref().is_some_and(|account| !account.is_empty_code_hash()) {
        return Err(BlockValidationError::msg(
            "EIP-8272 recent-root address has unexpected runtime code",
        )
        .into());
    }

    let mut account = current
        .map(Account::from)
        .unwrap_or_else(|| Account::new_not_existing(TransactionId::ZERO));
    account.info.nonce = account.info.nonce.max(1);
    account.info.code_hash = RECENT_ROOT_CODE_HASH;
    account.info.code = Some(Bytecode::new_legacy(RECENT_ROOT_CODE.clone()));
    account.mark_touch();
    db.commit([(RECENT_ROOT_ADDRESS, account)].into_iter().collect());
    Ok(())
}
