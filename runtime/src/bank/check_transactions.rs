use {
    super::{Bank, BankStatusCache},
    solana_accounts_db::blockhash_queue::BlockhashQueue,
    solana_perf::perf_libs,
    solana_sdk::{
        account::AccountSharedData,
        account_utils::StateMut,
        clock::{
            MAX_PROCESSING_AGE, MAX_TRANSACTION_FORWARDING_DELAY,
            MAX_TRANSACTION_FORWARDING_DELAY_GPU,
        },
        message::SanitizedMessage,
        nonce::{
            state::{
                Data as NonceData, DurableNonce, State as NonceState, Versions as NonceVersions,
            },
            NONCED_TX_MARKER_IX_INDEX,
        },
        nonce_account,
        pubkey::Pubkey,
        transaction::{Result as TransactionResult, SanitizedTransaction, TransactionError},
    },
    solana_svm::{
        account_loader::{CheckedTransactionDetails, TransactionCheckResult},
        nonce_info::NonceInfo,
        transaction_error_metrics::TransactionErrorMetrics,
    },
};

impl Bank {
    /// Checks a batch of sanitized transactions again bank for age and status
    pub fn check_transactions_with_forwarding_delay(
        &self,
        transactions: &[SanitizedTransaction],
        filter: &[TransactionResult<()>],
        forward_transactions_to_leader_at_slot_offset: u64,
    ) -> Vec<TransactionCheckResult> {
        let mut error_counters = TransactionErrorMetrics::default();
        // The following code also checks if the blockhash for a transaction is too old
        // The check accounts for
        //  1. Transaction forwarding delay
        //  2. The slot at which the next leader will actually process the transaction
        // Drop the transaction if it will expire by the time the next node receives and processes it
        let api = perf_libs::api();
        let max_tx_fwd_delay = if api.is_none() {
            MAX_TRANSACTION_FORWARDING_DELAY
        } else {
            MAX_TRANSACTION_FORWARDING_DELAY_GPU
        };

        self.check_transactions(
            transactions,
            filter,
            (MAX_PROCESSING_AGE)
                .saturating_sub(max_tx_fwd_delay)
                .saturating_sub(forward_transactions_to_leader_at_slot_offset as usize),
            &mut error_counters,
        )
    }

    pub fn check_transactions(
        &self,
        sanitized_txs: &[impl core::borrow::Borrow<SanitizedTransaction>],
        lock_results: &[TransactionResult<()>],
        max_age: usize,
        error_counters: &mut TransactionErrorMetrics,
    ) -> Vec<TransactionCheckResult> {
        let lock_results = self.check_age(sanitized_txs, lock_results, max_age, error_counters);
        self.check_status_cache(sanitized_txs, lock_results, error_counters)
    }

    fn check_age(
        &self,
        sanitized_txs: &[impl core::borrow::Borrow<SanitizedTransaction>],
        lock_results: &[TransactionResult<()>],
        max_age: usize,
        error_counters: &mut TransactionErrorMetrics,
    ) -> Vec<TransactionCheckResult> {
        let hash_queue = self.blockhash_queue.read().unwrap();
        let last_blockhash = hash_queue.last_hash();
        let next_durable_nonce = DurableNonce::from_blockhash(&last_blockhash);
        // safe so long as the BlockhashQueue is consistent
        let next_lamports_per_signature = hash_queue
            .get_lamports_per_signature(&last_blockhash)
            .unwrap();

        sanitized_txs
            .iter()
            .zip(lock_results)
            .map(|(tx, lock_res)| match lock_res {
                Ok(()) => self.check_transaction_age(
                    tx.borrow(),
                    max_age,
                    &next_durable_nonce,
                    &hash_queue,
                    next_lamports_per_signature,
                    error_counters,
                ),
                Err(e) => Err(e.clone()),
            })
            .collect()
    }

    fn check_transaction_age(
        &self,
        tx: &SanitizedTransaction,
        max_age: usize,
        next_durable_nonce: &DurableNonce,
        hash_queue: &BlockhashQueue,
        next_lamports_per_signature: u64,
        error_counters: &mut TransactionErrorMetrics,
    ) -> TransactionCheckResult {
        let recent_blockhash = tx.message().recent_blockhash();
        if let Some(hash_info) = hash_queue.get_hash_info_if_valid(recent_blockhash, max_age) {
            Ok(CheckedTransactionDetails {
                nonce: None,
                lamports_per_signature: hash_info.lamports_per_signature(),
            })
        } else if let Some((nonce, previous_lamports_per_signature)) = self
            .check_load_and_advance_message_nonce_account(
                tx.message(),
                next_durable_nonce,
                next_lamports_per_signature,
            )
        {
            Ok(CheckedTransactionDetails {
                nonce: Some(nonce),
                lamports_per_signature: previous_lamports_per_signature,
            })
        } else {
            error_counters.blockhash_not_found += 1;
            Err(TransactionError::BlockhashNotFound)
        }
    }

    pub(super) fn check_load_and_advance_message_nonce_account(
        &self,
        message: &SanitizedMessage,
        next_durable_nonce: &DurableNonce,
        next_lamports_per_signature: u64,
    ) -> Option<(NonceInfo, u64)> {
        let nonce_is_advanceable = message.recent_blockhash() != next_durable_nonce.as_hash();
        if !nonce_is_advanceable {
            return None;
        }

        let (nonce_address, mut nonce_account, nonce_data) =
            self.load_message_nonce_account(message)?;

        let previous_lamports_per_signature = nonce_data.get_lamports_per_signature();
        let next_nonce_state = NonceState::new_initialized(
            &nonce_data.authority,
            *next_durable_nonce,
            next_lamports_per_signature,
        );
        nonce_account
            .set_state(&NonceVersions::new(next_nonce_state))
            .ok()?;

        Some((
            NonceInfo::new(nonce_address, nonce_account),
            previous_lamports_per_signature,
        ))
    }

    pub(super) fn load_message_nonce_account(
        &self,
        message: &SanitizedMessage,
    ) -> Option<(Pubkey, AccountSharedData, NonceData)> {
        let nonce_address = message.get_durable_nonce()?;
        let nonce_account = self.get_account_with_fixed_root(nonce_address)?;
        let nonce_data =
            nonce_account::verify_nonce_account(&nonce_account, message.recent_blockhash())?;

        let nonce_is_authorized = message
            .get_ix_signers(NONCED_TX_MARKER_IX_INDEX as usize)
            .any(|signer| signer == &nonce_data.authority);
        if !nonce_is_authorized {
            return None;
        }

        Some((*nonce_address, nonce_account, nonce_data))
    }

    fn check_status_cache(
        &self,
        sanitized_txs: &[impl core::borrow::Borrow<SanitizedTransaction>],
        lock_results: Vec<TransactionCheckResult>,
        error_counters: &mut TransactionErrorMetrics,
    ) -> Vec<TransactionCheckResult> {
        let rcache = self.status_cache.read().unwrap();
        sanitized_txs
            .iter()
            .zip(lock_results)
            .map(|(sanitized_tx, lock_result)| {
                let sanitized_tx = sanitized_tx.borrow();
                if lock_result.is_ok()
                    && self.is_transaction_already_processed(sanitized_tx, &rcache)
                {
                    error_counters.already_processed += 1;
                    return Err(TransactionError::AlreadyProcessed);
                }

                lock_result
            })
            .collect()
    }

    fn is_transaction_already_processed(
        &self,
        sanitized_tx: &SanitizedTransaction,
        status_cache: &BankStatusCache,
    ) -> bool {
        let key = sanitized_tx.message_hash();
        let transaction_blockhash = sanitized_tx.message().recent_blockhash();
        status_cache
            .get_status(key, transaction_blockhash, &self.ancestors)
            .is_some()
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::bank::tests::{
            get_nonce_blockhash, get_nonce_data_from_account, new_sanitized_message,
            setup_nonce_with_bank,
        },
        solana_sdk::{
            feature_set::FeatureSet, hash::Hash, message::Message, signature::Keypair,
            signer::Signer, system_instruction,
        },
    };

    #[test]
    fn test_check_and_load_message_nonce_account_ok() {
        const STALE_LAMPORTS_PER_SIGNATURE: u64 = 42;
        let (bank, _mint_keypair, custodian_keypair, nonce_keypair, _) = setup_nonce_with_bank(
            10_000_000,
            |_| {},
            5_000_000,
            250_000,
            None,
            FeatureSet::all_enabled(),
        )
        .unwrap();
        let custodian_pubkey = custodian_keypair.pubkey();
        let nonce_pubkey = nonce_keypair.pubkey();

        let nonce_hash = get_nonce_blockhash(&bank, &nonce_pubkey).unwrap();
        let message = new_sanitized_message(Message::new_with_blockhash(
            &[
                system_instruction::advance_nonce_account(&nonce_pubkey, &nonce_pubkey),
                system_instruction::transfer(&custodian_pubkey, &nonce_pubkey, 100_000),
            ],
            Some(&custodian_pubkey),
            &nonce_hash,
        ));

        // set a spurious lamports_per_signature value
        let mut nonce_account = bank.get_account(&nonce_pubkey).unwrap();
        let nonce_data = get_nonce_data_from_account(&nonce_account).unwrap();
        nonce_account
            .set_state(&NonceVersions::new(NonceState::new_initialized(
                &nonce_data.authority,
                nonce_data.durable_nonce,
                STALE_LAMPORTS_PER_SIGNATURE,
            )))
            .unwrap();
        bank.store_account(&nonce_pubkey, &nonce_account);

        let nonce_account = bank.get_account(&nonce_pubkey).unwrap();
        let (_, next_lamports_per_signature) = bank.last_blockhash_and_lamports_per_signature();
        let mut expected_nonce_info = NonceInfo::new(nonce_pubkey, nonce_account);
        expected_nonce_info
            .try_advance_nonce(bank.next_durable_nonce(), next_lamports_per_signature)
            .unwrap();

        // we now expect to:
        // * advance the nonce account to the current durable nonce value
        // * set the blockhash queue's last blockhash's lamports_per_signature value in the nonce data
        // * retrieve the previous lamports_per_signature value set on the nonce data for transaction fee checks
        assert_eq!(
            bank.check_load_and_advance_message_nonce_account(
                &message,
                &bank.next_durable_nonce(),
                next_lamports_per_signature
            ),
            Some((expected_nonce_info, STALE_LAMPORTS_PER_SIGNATURE)),
        );
    }

    #[test]
    fn test_check_and_load_message_nonce_account_not_nonce_fail() {
        let (bank, _mint_keypair, custodian_keypair, nonce_keypair, _) = setup_nonce_with_bank(
            10_000_000,
            |_| {},
            5_000_000,
            250_000,
            None,
            FeatureSet::all_enabled(),
        )
        .unwrap();
        let custodian_pubkey = custodian_keypair.pubkey();
        let nonce_pubkey = nonce_keypair.pubkey();

        let nonce_hash = get_nonce_blockhash(&bank, &nonce_pubkey).unwrap();
        let message = new_sanitized_message(Message::new_with_blockhash(
            &[
                system_instruction::transfer(&custodian_pubkey, &nonce_pubkey, 100_000),
                system_instruction::advance_nonce_account(&nonce_pubkey, &nonce_pubkey),
            ],
            Some(&custodian_pubkey),
            &nonce_hash,
        ));
        let (_, lamports_per_signature) = bank.last_blockhash_and_lamports_per_signature();
        assert!(bank
            .check_load_and_advance_message_nonce_account(
                &message,
                &bank.next_durable_nonce(),
                lamports_per_signature
            )
            .is_none());
    }

    #[test]
    fn test_check_and_load_message_nonce_account_missing_ix_pubkey_fail() {
        let (bank, _mint_keypair, custodian_keypair, nonce_keypair, _) = setup_nonce_with_bank(
            10_000_000,
            |_| {},
            5_000_000,
            250_000,
            None,
            FeatureSet::all_enabled(),
        )
        .unwrap();
        let custodian_pubkey = custodian_keypair.pubkey();
        let nonce_pubkey = nonce_keypair.pubkey();

        let nonce_hash = get_nonce_blockhash(&bank, &nonce_pubkey).unwrap();
        let mut message = Message::new_with_blockhash(
            &[
                system_instruction::advance_nonce_account(&nonce_pubkey, &nonce_pubkey),
                system_instruction::transfer(&custodian_pubkey, &nonce_pubkey, 100_000),
            ],
            Some(&custodian_pubkey),
            &nonce_hash,
        );
        message.instructions[0].accounts.clear();
        let (_, lamports_per_signature) = bank.last_blockhash_and_lamports_per_signature();
        assert!(bank
            .check_load_and_advance_message_nonce_account(
                &new_sanitized_message(message),
                &bank.next_durable_nonce(),
                lamports_per_signature,
            )
            .is_none());
    }

    #[test]
    fn test_check_and_load_message_nonce_account_nonce_acc_does_not_exist_fail() {
        let (bank, _mint_keypair, custodian_keypair, nonce_keypair, _) = setup_nonce_with_bank(
            10_000_000,
            |_| {},
            5_000_000,
            250_000,
            None,
            FeatureSet::all_enabled(),
        )
        .unwrap();
        let custodian_pubkey = custodian_keypair.pubkey();
        let nonce_pubkey = nonce_keypair.pubkey();
        let missing_keypair = Keypair::new();
        let missing_pubkey = missing_keypair.pubkey();

        let nonce_hash = get_nonce_blockhash(&bank, &nonce_pubkey).unwrap();
        let message = new_sanitized_message(Message::new_with_blockhash(
            &[
                system_instruction::advance_nonce_account(&missing_pubkey, &nonce_pubkey),
                system_instruction::transfer(&custodian_pubkey, &nonce_pubkey, 100_000),
            ],
            Some(&custodian_pubkey),
            &nonce_hash,
        ));
        let (_, lamports_per_signature) = bank.last_blockhash_and_lamports_per_signature();
        assert!(bank
            .check_load_and_advance_message_nonce_account(
                &message,
                &bank.next_durable_nonce(),
                lamports_per_signature
            )
            .is_none());
    }

    #[test]
    fn test_check_and_load_message_nonce_account_bad_tx_hash_fail() {
        let (bank, _mint_keypair, custodian_keypair, nonce_keypair, _) = setup_nonce_with_bank(
            10_000_000,
            |_| {},
            5_000_000,
            250_000,
            None,
            FeatureSet::all_enabled(),
        )
        .unwrap();
        let custodian_pubkey = custodian_keypair.pubkey();
        let nonce_pubkey = nonce_keypair.pubkey();

        let message = new_sanitized_message(Message::new_with_blockhash(
            &[
                system_instruction::advance_nonce_account(&nonce_pubkey, &nonce_pubkey),
                system_instruction::transfer(&custodian_pubkey, &nonce_pubkey, 100_000),
            ],
            Some(&custodian_pubkey),
            &Hash::default(),
        ));
        let (_, lamports_per_signature) = bank.last_blockhash_and_lamports_per_signature();
        assert!(bank
            .check_load_and_advance_message_nonce_account(
                &message,
                &bank.next_durable_nonce(),
                lamports_per_signature
            )
            .is_none());
    }
}
