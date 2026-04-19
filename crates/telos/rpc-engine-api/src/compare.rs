use std::{collections::HashSet, fmt::Display};

use alloy_consensus::constants::KECCAK_EMPTY;
use alloy_primitives::{Address, Bytes, B256, U256};
use revm::{
    bytecode::Bytecode,
    database::State,
    primitives::AddressMap,
    state::{Account, AccountInfo, EvmStorage, EvmStorageSlot},
    Database, DatabaseCommit,
};
use sha2::{Digest, Sha256};
use tracing::{debug, warn};

use crate::structs::{TelosAccountStateTableRow, TelosAccountTableRow};

struct StateOverride {
    accounts: AddressMap<Account>,
}

impl StateOverride {
    pub fn new() -> Self {
        StateOverride { accounts: AddressMap::default() }
    }

    fn maybe_init_account<DB: Database>(&mut self, revm_db: &mut State<DB>, address: Address) {
        if self.accounts.contains_key(&address) {
            return;
        }
        let info = match revm_db.basic(address) {
            Ok(maybe_info) => maybe_info.unwrap_or_default(),
            Err(_) => AccountInfo::default(),
        };

        let mut acc = Account::default();
        acc.info = info;
        // Mark as InMemoryChange so revm state persistence picks up these changes.
        // Account::default() has status LoadedNotExisting which would be treated as unmodified.
        acc.mark_touch();
        self.accounts.insert(address, acc);
    }

    pub fn override_account<DB: Database>(
        &mut self,
        revm_db: &mut State<DB>,
        telos_row: &TelosAccountTableRow,
    ) {
        self.maybe_init_account(revm_db, telos_row.address);
        let acc = self.accounts.get_mut(&telos_row.address).unwrap();
        acc.info.balance = telos_row.balance;
        acc.info.nonce = telos_row.nonce;
        acc.mark_touch();
        if !telos_row.code.is_empty() {
            acc.info.code_hash =
                B256::from_slice(Sha256::digest(telos_row.code.as_ref()).as_slice());
            acc.info.code = Some(Bytecode::new_legacy(telos_row.code.clone()));
        } else {
            acc.info.code_hash = KECCAK_EMPTY;
            acc.info.code = None;
        }
    }

    pub fn override_balance<DB: Database>(
        &mut self,
        revm_db: &mut State<DB>,
        address: Address,
        balance: U256,
    ) {
        self.maybe_init_account(revm_db, address);
        let acc = self.accounts.get_mut(&address).unwrap();
        acc.info.balance = balance;
    }

    pub fn override_nonce<DB: Database>(
        &mut self,
        revm_db: &mut State<DB>,
        address: Address,
        nonce: u64,
    ) {
        self.maybe_init_account(revm_db, address);
        let acc = self.accounts.get_mut(&address).unwrap();
        acc.info.nonce = nonce;
    }

    pub fn override_code<DB: Database>(
        &mut self,
        revm_db: &mut State<DB>,
        address: Address,
        maybe_code: &Bytes,
    ) {
        self.maybe_init_account(revm_db, address);
        let acc = self.accounts.get_mut(&address).unwrap();
        if !maybe_code.is_empty() {
            acc.info.code_hash = B256::from_slice(Sha256::digest(maybe_code.as_ref()).as_slice());
            acc.info.code = Some(Bytecode::new_legacy(maybe_code.clone()));
        } else {
            acc.info.code_hash = KECCAK_EMPTY;
            acc.info.code = None;
        }
    }

    pub fn override_storage<DB: Database>(
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

    pub fn apply<DB: Database>(&self, revm_db: &mut State<DB>) {
        revm_db.commit(self.accounts.clone());
    }
}

macro_rules! maybe_panic {
    ($panic_mode:expr, $($arg:tt)*) => {
        if $panic_mode {
            panic!($($arg)*);
        } else {
            warn!($($arg)*);
        }
    };
}

/// Compare state diffs between revm execution and Telos EVM contract state.
///
/// This function validates that the execution results from revm match what the
/// Telos native EVM contract reports. Any discrepancies are logged (or panicked
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
        if new_addresses_using_openwallet_hashset.contains(&row.address) &&
            row.balance == U256::ZERO &&
            row.nonce == 0 &&
            row.code.is_empty()
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
                if revm_row != row.value {
                    if revm_row != U256::ZERO && row.removed {
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
                    if !row.removed {
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
                }
            } else {
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
