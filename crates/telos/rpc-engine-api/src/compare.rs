use std::{collections::HashSet, fmt::Display};

use alloy_consensus::constants::KECCAK_EMPTY;
use alloy_primitives::{Address, Bytes, B256, U256};
use revm::{
    bytecode::Bytecode,
    database::State,
    primitives::AddressMap,
    state::{Account, AccountInfo, EvmStorageSlot},
    Database, DatabaseCommit,
};
use sha2::{Digest, Sha256};
use tracing::debug;

use crate::structs::{TelosAccountStateTableRow, TelosAccountTableRow};

struct StateOverride {
    accounts: AddressMap<Account>,
}

impl StateOverride {
    pub(crate) fn new() -> Self {
        Self { accounts: AddressMap::default() }
    }

    fn maybe_init_account<DB: Database>(&mut self, revm_db: &mut State<DB>, address: Address) {
        if self.accounts.contains_key(&address) {
            return;
        }
        let info = match revm_db.basic(address) {
            Ok(maybe_info) => maybe_info.unwrap_or_default(),
            Err(_) => AccountInfo::default(),
        };

        let mut acc = Account { info, ..Account::default() };
        // Mark as InMemoryChange so revm state persistence picks up these changes.
        // Account::default() has status LoadedNotExisting which would be treated as unmodified.
        acc.mark_touch();
        self.accounts.insert(address, acc);
    }

    pub(crate) fn override_account<DB: Database>(
        &mut self,
        revm_db: &mut State<DB>,
        telos_row: &TelosAccountTableRow,
    ) {
        self.maybe_init_account(revm_db, telos_row.address);
        let acc = self.accounts.get_mut(&telos_row.address).unwrap();
        acc.info.balance = telos_row.balance;
        acc.info.nonce = telos_row.nonce;
        acc.mark_touch();
        if telos_row.code.is_empty() {
            acc.info.code_hash = KECCAK_EMPTY;
            acc.info.code = None;
        } else {
            acc.info.code_hash =
                B256::from_slice(Sha256::digest(telos_row.code.as_ref()).as_slice());
            acc.info.code = Some(Bytecode::new_legacy(telos_row.code.clone()));
        }
    }

    pub(crate) fn override_balance<DB: Database>(
        &mut self,
        revm_db: &mut State<DB>,
        address: Address,
        balance: U256,
    ) {
        self.maybe_init_account(revm_db, address);
        let acc = self.accounts.get_mut(&address).unwrap();
        acc.info.balance = balance;
    }

    pub(crate) fn override_nonce<DB: Database>(
        &mut self,
        revm_db: &mut State<DB>,
        address: Address,
        nonce: u64,
    ) {
        self.maybe_init_account(revm_db, address);
        let acc = self.accounts.get_mut(&address).unwrap();
        acc.info.nonce = nonce;
    }

    pub(crate) fn override_code<DB: Database>(
        &mut self,
        revm_db: &mut State<DB>,
        address: Address,
        maybe_code: &Bytes,
    ) {
        self.maybe_init_account(revm_db, address);
        let acc = self.accounts.get_mut(&address).unwrap();
        if maybe_code.is_empty() {
            acc.info.code_hash = KECCAK_EMPTY;
            acc.info.code = None;
        } else {
            acc.info.code_hash = B256::from_slice(Sha256::digest(maybe_code.as_ref()).as_slice());
            acc.info.code = Some(Bytecode::new_legacy(maybe_code.clone()));
        }
    }

    pub(crate) fn override_storage<DB: Database>(
        &mut self,
        revm_db: &mut State<DB>,
        address: Address,
        key: U256,
        new_val: U256,
        old_val: U256,
    ) {
        self.maybe_init_account(revm_db, address);
        let acc = self.accounts.get_mut(&address).unwrap();
        acc.storage.insert(key, EvmStorageSlot::new_changed(old_val, new_val, 0));
    }

    pub(crate) fn apply<DB: Database>(&self, revm_db: &mut State<DB>) {
        revm_db.commit(self.accounts.clone());
    }
}

macro_rules! maybe_panic {
    ($panic_mode:expr, $($arg:tt)*) => {
        if $panic_mode {
            panic!($($arg)*);
        } else {
            debug!($($arg)*);
        }
    };
}

/// Compare state diffs between revm execution and Telos EVM contract state.
///
/// This function validates that the local revm state matches what the Telos
/// native EVM contract reports. Any discrepancies are debug-logged (or panicked
/// in strict mode) and overridden to match the Telos state.
pub fn compare_state_diffs<DB>(
    revm_db: &mut State<DB>,
    statediffs_account: Vec<TelosAccountTableRow>,
    statediffs_accountstate: Vec<TelosAccountStateTableRow>,
    _new_addresses_using_create: Vec<(u64, U256)>,
    new_addresses_using_openwallet: Vec<(u64, U256)>,
    panic_mode: bool,
    do_storage: bool,
) -> bool
where
    DB: Database,
    DB::Error: Display,
{
    let mut state_override = StateOverride::new();

    let new_addresses_using_openwallet_hashset: HashSet<Address> = new_addresses_using_openwallet
        .iter()
        .map(|row| Address::from_word(B256::from(row.1)))
        .collect();

    for row in &statediffs_account {
        // Skip addresses created via openwallet with zero state
        if new_addresses_using_openwallet_hashset.contains(&row.address)
            && row.balance == U256::ZERO
            && row.nonce == 0
            && row.code.is_empty()
        {
            continue;
        }
        if row.removed {
            continue;
        }
        if let Ok(revm_row) = revm_db.basic(row.address) {
            if let Some(unwrapped) = revm_row {
                if unwrapped.balance != row.balance {
                    maybe_panic!(
                        panic_mode,
                        "Difference in balance, address: {:?} - revm: {:?} - tevm: {:?}",
                        row.address,
                        unwrapped.balance,
                        row.balance
                    );
                    state_override.override_balance(revm_db, row.address, row.balance);
                }
                if unwrapped.nonce != row.nonce {
                    maybe_panic!(
                        panic_mode,
                        "Difference in nonce, address: {:?} - revm: {:?} - tevm: {:?}",
                        row.address,
                        unwrapped.nonce,
                        row.nonce
                    );
                    state_override.override_nonce(revm_db, row.address, row.nonce);
                }
                match unwrapped.code.clone() {
                    None => {
                        if !row.code.is_empty() {
                            maybe_panic!(
                                panic_mode,
                                "Difference in code existence, address: {:?}",
                                row.address
                            );
                            state_override.override_code(revm_db, row.address, &row.code);
                        }
                    }
                    Some(revm_bytecode) => {
                        if revm_bytecode.len() != row.code.len() {
                            maybe_panic!(
                                panic_mode,
                                "Difference in code size, address: {:?} - revm: {:?} - tevm: {:?}",
                                row.address,
                                revm_bytecode.len(),
                                row.code.len()
                            );
                            state_override.override_code(revm_db, row.address, &row.code);
                        }
                    }
                }
            } else if !(row.balance == U256::ZERO && row.nonce == 0 && row.code.is_empty()) {
                maybe_panic!(
                    panic_mode,
                    "A modified account table row was not found on revm state, address: {:?}",
                    row.address
                );
                state_override.override_account(revm_db, row);
            }
        } else if !(row.balance == U256::ZERO && row.nonce == 0 && row.code.is_empty()) {
            maybe_panic!(
                panic_mode,
                "A modified account table row was not found on revm state, address: {:?}",
                row.address
            );
            state_override.override_account(revm_db, row);
        }
    }

    if do_storage {
        for row in &statediffs_accountstate {
            if let Ok(revm_row) = revm_db.storage(row.address, row.key) {
                if row.removed {
                    if revm_row != U256::ZERO {
                        maybe_panic!(
                            panic_mode,
                            "Difference in value on revm storage, removed on Telos, address: {:?}, key: {:?}",
                            row.address,
                            row.key
                        );
                        state_override.override_storage(
                            revm_db,
                            row.address,
                            row.key,
                            U256::ZERO,
                            revm_row,
                        );
                    }
                } else if revm_row != row.value {
                    maybe_panic!(
                        panic_mode,
                        "Difference in value on revm storage, address: {:?}, key: {:?}",
                        row.address,
                        row.key
                    );
                    state_override.override_storage(
                        revm_db,
                        row.address,
                        row.key,
                        row.value,
                        revm_row,
                    );
                }
            } else if !row.removed {
                maybe_panic!(
                    panic_mode,
                    "Key was not found on revm storage, address: {:?}, key: {:?}",
                    row.address,
                    row.key
                );
                state_override.override_storage(
                    revm_db,
                    row.address,
                    row.key,
                    row.value,
                    U256::ZERO,
                );
            }
        }
    }

    state_override.apply(revm_db);

    debug!("State diff comparison complete");
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::address;
    use revm::{
        database::{CacheDB, EmptyDB},
        state::AccountInfo,
    };

    fn state_with_storage(address: Address, key: U256, value: U256) -> State<CacheDB<EmptyDB>> {
        let mut db = CacheDB::<EmptyDB>::default();
        db.insert_account_info(address, AccountInfo { nonce: 1, ..AccountInfo::default() });
        db.insert_account_storage(address, key, value).unwrap();
        State::builder().with_database(db).with_bundle_update().build()
    }

    #[test]
    fn removed_storage_row_clears_even_when_value_is_old_value() {
        let address = address!("0x1000000000000000000000000000000000000000");
        let key = U256::ZERO;
        let old_value = U256::from(1);
        let mut state = state_with_storage(address, key, old_value);

        assert_eq!(state.storage(address, key).unwrap(), old_value);

        compare_state_diffs(
            &mut state,
            Vec::new(),
            vec![TelosAccountStateTableRow { removed: true, address, key, value: old_value }],
            Vec::new(),
            Vec::new(),
            false,
            true,
        );

        assert_eq!(state.storage(address, key).unwrap(), U256::ZERO);
    }

    #[test]
    fn removed_zero_storage_row_is_noop() {
        let address = address!("0x2000000000000000000000000000000000000000");
        let key = U256::ZERO;
        let mut db = CacheDB::<EmptyDB>::default();
        db.insert_account_info(address, AccountInfo::default());
        let mut state = State::builder().with_database(db).with_bundle_update().build();

        compare_state_diffs(
            &mut state,
            Vec::new(),
            vec![TelosAccountStateTableRow { removed: true, address, key, value: U256::from(1) }],
            Vec::new(),
            Vec::new(),
            false,
            true,
        );

        assert_eq!(state.storage(address, key).unwrap(), U256::ZERO);
    }

    #[test]
    fn non_removed_storage_row_still_sets_value() {
        let address = address!("0x3000000000000000000000000000000000000000");
        let key = U256::ZERO;
        let mut state = state_with_storage(address, key, U256::from(1));
        let new_value = U256::from(7);

        compare_state_diffs(
            &mut state,
            Vec::new(),
            vec![TelosAccountStateTableRow { removed: false, address, key, value: new_value }],
            Vec::new(),
            Vec::new(),
            false,
            true,
        );

        assert_eq!(state.storage(address, key).unwrap(), new_value);
    }
}
