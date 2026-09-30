//! EIP-8250 nonce manager activation.

use crate::{
    block::{BlockExecutionError, StateDB},
    Evm,
};
use alloy_eips::eip8141::{NONCE_MANAGER, NONCE_MANAGER_CODE};
use alloy_primitives::{keccak256, Bytes, KECCAK256_EMPTY};
use revm::{
    state::{Account, Bytecode, TransactionId},
    Database, DatabaseCommit,
};

/// Installs the nonce manager without replacing existing code or resetting its state.
pub(crate) fn install_nonce_manager(
    evm: &mut impl Evm<DB: StateDB>,
) -> Result<(), BlockExecutionError> {
    let db = evm.db_mut();
    let code_hash = keccak256(NONCE_MANAGER_CODE);
    let current = db.basic(NONCE_MANAGER).map_err(BlockExecutionError::other)?;
    // The reverting runtime cannot be replaced or destroyed; its presence marks completed
    // activation.
    if current.as_ref().is_some_and(|account| account.code_hash == code_hash) {
        return Ok(());
    }
    if current.as_ref().is_some_and(|account| account.code_hash != KECCAK256_EMPTY) {
        return Err(BlockExecutionError::msg("EIP-8250 nonce manager address contains code"));
    }
    let mut account = current
        .map(Account::from)
        .unwrap_or_else(|| Account::new_not_existing(TransactionId::ZERO));
    account.info.nonce = account.info.nonce.max(1);
    account.info.code_hash = code_hash;
    account.info.code = Some(Bytecode::new_legacy(Bytes::copy_from_slice(&NONCE_MANAGER_CODE)));
    account.mark_touch();
    db.commit([(NONCE_MANAGER, account)].into_iter().collect());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{eth::EthEvmFactory, EvmEnv, EvmFactory};
    use alloy_primitives::U256;
    use revm::{
        database::{CacheDB, EmptyDB},
        state::AccountInfo,
    };

    #[test]
    fn activation_follows_eip8141() {
        use alloy_hardforks::{EthereumHardfork, EthereumHardforks, ForkCondition};
        struct Spec;
        impl EthereumHardforks for Spec {
            fn ethereum_fork_activation(&self, _: EthereumHardfork) -> ForkCondition {
                ForkCondition::Timestamp(10)
            }
        }
        let mut caller = super::super::SystemCaller::new(Spec);
        let mut db = CacheDB::<EmptyDB>::default();
        for timestamp in [9, 10, 11] {
            let mut evm = EthEvmFactory
                .create_evm(db, EvmEnv::default().with_timestamp(U256::from(timestamp)));
            caller.apply_eip8141_fork_transition(&mut evm).unwrap();
            let account = evm.db_mut().basic(NONCE_MANAGER).unwrap();
            assert_eq!(account.is_some(), timestamp >= 10);
            let verifier = evm.db_mut().basic(alloy_eips::eip8141::EXPIRY_VERIFIER).unwrap();
            assert_eq!(verifier.is_some(), timestamp >= 10);
            db = evm.into_db();
        }
    }

    #[test]
    fn activation_preserves_balance_nonce_and_subsequent_storage() {
        for existing_nonce in [0, 7] {
            let mut db = CacheDB::<EmptyDB>::default();
            db.insert_account_info(
                NONCE_MANAGER,
                AccountInfo {
                    nonce: existing_nonce,
                    balance: U256::from(19),
                    ..Default::default()
                },
            );
            let mut evm = EthEvmFactory.create_evm(db, EvmEnv::default());
            install_nonce_manager(&mut evm).unwrap();
            let info = evm.db_mut().basic(NONCE_MANAGER).unwrap().unwrap();
            assert_eq!(info.nonce, existing_nonce.max(1));
            assert_eq!(info.balance, U256::from(19));
            assert_eq!(info.code_hash, keccak256(NONCE_MANAGER_CODE));
            evm.db_mut()
                .insert_account_storage(NONCE_MANAGER, U256::from(42), U256::from(3))
                .unwrap();
            install_nonce_manager(&mut evm).unwrap();
            assert_eq!(evm.db_mut().basic(NONCE_MANAGER).unwrap().unwrap(), info);
            assert_eq!(evm.db_mut().storage(NONCE_MANAGER, U256::from(42)).unwrap(), U256::from(3));
        }
    }

    #[test]
    fn activation_rejects_existing_code() {
        let mut db = CacheDB::<EmptyDB>::default();
        let info =
            AccountInfo::default().with_code(Bytecode::new_legacy(Bytes::from_static(&[0x00])));
        db.insert_account_info(NONCE_MANAGER, info.clone());
        let mut evm = EthEvmFactory.create_evm(db, EvmEnv::default());
        assert!(install_nonce_manager(&mut evm).is_err());
        assert_eq!(evm.db_mut().basic(NONCE_MANAGER).unwrap().unwrap(), info);
    }
}
