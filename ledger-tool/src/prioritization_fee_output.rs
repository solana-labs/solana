use {
    lazy_static::lazy_static,
    solana_runtime::transaction_priority_details::{
        GetTransactionPriorityDetails, TransactionPriorityDetails,
    },
    solana_sdk::{
        clock::Slot, pubkey::Pubkey, saturating_add_assign, signature::Signature,
        transaction::SanitizedTransaction,
    },
    std::{
        collections::{HashMap, VecDeque},
        str::FromStr,
    },
};

pub enum PrioritizationFeeMarketAnomalies {
    HigherPrioritiedTxIsBehind,
    /* other possible anomaly to add later
    vote_set_prioritization,
    set_cu_price_without_setting_cu_limit,
    using_deprecated_COMPUTE_BUDGET_IX,
    set_cu_limit_above_max,
    set_cu_price_without_congestion,
    // */
}

#[derive(Default, Debug)]
struct PriorityFeeMarketEntry {
    entry_index: u64,
    transaction_index: u64,
    prioritization_fee: u64,
    cu_limit: u64,
    tx_signature: Signature,
    is_vote: bool,
}

type PriorityFeeMarket = VecDeque<PriorityFeeMarketEntry>;

#[derive(Default, Debug)]
struct Summary {
    number_of_transactions: usize,
    number_of_non_vote_transactions: usize,
    number_of_prioritized_transactions: usize,
    total_prioritization_fee: usize,
}

pub(crate) struct PriorityFeeBySlot {
    slot: Slot,
    local_markets: HashMap<Pubkey, PriorityFeeMarket>,
    summary: Summary,
}

lazy_static! {
    // create a special pubkey to represent block priofee market
    pub static ref BLOCK_FEE_MARKET_KEY: Pubkey = Pubkey::from_str("b1oc5e8LSDfpkcHZFE1QcDSqDc8ZRjLDMLwyfRL6nCC").unwrap();
}

impl PriorityFeeBySlot {
    pub fn new(slot: Slot) -> Self {
        // there always be a fee market for block itself
        let local_markets = HashMap::from([(*BLOCK_FEE_MARKET_KEY, PriorityFeeMarket::default())]);

        PriorityFeeBySlot {
            slot,
            local_markets,
            summary: Summary::default(),
        }
    }

    pub fn insert(
        &mut self,
        entry_index: u64,
        transaction_index: u64,
        sanitized_transaction: &SanitizedTransaction,
    ) {
        let is_vote = sanitized_transaction.is_simple_vote_transaction();
        let priority_details = sanitized_transaction.get_transaction_priority_details(false); // round compute unit price is not enabled
        let account_locks = sanitized_transaction.get_account_locks(256); // hardcode to avoid using `bank` here
        if priority_details.is_none() || account_locks.is_err() {
            println!(
                "ERROR: fail to get priority details or account locks for tx {:?}",
                sanitized_transaction
            );
            return;
        }
        let priority_details = priority_details.unwrap();
        let local_market_keys = account_locks
            .unwrap()
            .writable
            .iter()
            .map(|key| **key)
            .chain(std::iter::once(*BLOCK_FEE_MARKET_KEY))
            .collect::<Vec<_>>();

        for local_market_key in local_market_keys.iter() {
            let local_market = self
                .local_markets
                .entry(*local_market_key)
                .or_insert(PriorityFeeMarket::default());

            local_market.push_back(PriorityFeeMarketEntry {
                entry_index,
                transaction_index,
                prioritization_fee: priority_details.priority,
                cu_limit: priority_details.compute_unit_limit,
                tx_signature: *sanitized_transaction.signature(),
                is_vote,
            });
        }

        saturating_add_assign!(self.summary.number_of_transactions, 1);
        if !is_vote {
            saturating_add_assign!(self.summary.number_of_non_vote_transactions, 1);
        }
        if priority_details.priority > 0 {
            saturating_add_assign!(self.summary.number_of_prioritized_transactions, 1);
        }
        saturating_add_assign!(
            self.summary.total_prioritization_fee,
            Self::calculate_priority_fee_in_lamports(&priority_details) as usize
        );
    }

    pub fn print(&self, verbose_level: u64) {
        // if there is only one tx in write-locked accounts, it does not creates a local fee
        // market. Count them separately.
        let number_of_write_locked_accounts = self.local_markets.len();
        let number_of_local_markets = self
            .local_markets
            .iter()
            .filter(|(_key, market)| market.len() > 1)
            .count();
        let anomaly_markets = self
            .local_markets
            .iter()
            .filter_map(|(key, market)| {
                if Self::scan_market(market).is_err() {
                    return Some(*key);
                }
                None
            })
            .collect::<Vec<_>>();
        let number_of_anomaly_local_markets = anomaly_markets.len();

        println!(
            "==== Prioritization Fee Summary: \
                 slot {}, \
                 number of transactions {}, \
                 number of non-vote transactions {}, \
                 number of prioritized transactions {}, \
                 total prioritization fee {}, \
                 number of write-locked accounts {}, \
                 number of local fee markets {}, \
                 number of anomaly local markets {}, \
                 ",
            self.slot,
            self.summary.number_of_transactions,
            self.summary.number_of_non_vote_transactions,
            self.summary.number_of_prioritized_transactions,
            self.summary.total_prioritization_fee,
            number_of_write_locked_accounts,
            number_of_local_markets,
            number_of_anomaly_local_markets,
        );

        let include_vote_tx: bool = verbose_level >= 3;
        if verbose_level >= 1 {
            println!(
                "---- block space fee market, include vote = {} ----",
                include_vote_tx
            );
            self.print_market(&BLOCK_FEE_MARKET_KEY, include_vote_tx);
            println!("");
        }

        if verbose_level >= 2 {
            for anomaly_market_key in anomaly_markets.iter() {
                if *anomaly_market_key != *BLOCK_FEE_MARKET_KEY {
                    println!("---- anomaly local fee market ----");
                    self.print_market(anomaly_market_key, include_vote_tx);
                    println!("");
                }
            }
        }

        if verbose_level >= 3 {
            for (key, _market) in self.local_markets.iter() {
                if !anomaly_markets.contains(key) {
                    println!("---- rest local fee market ----");
                    self.print_market(key, include_vote_tx);
                    println!("");
                }
            }
        }
    }

    fn print_market(&self, market_key: &Pubkey, include_vote_tx: bool) {
        println!("\tlocal market key: {:?}", market_key);
        println!("\tentry_index\ttx_index\tpriority_fee\tcu_limit\ttx_signature\tis_vote");
        for entry in self.local_markets.get(market_key).unwrap().iter() {
            if include_vote_tx || !entry.is_vote {
                println!(
                    "\t{:?}\t{:?}\t{:?}\t{:?}\t{:?} \t{:?}",
                    entry.entry_index,
                    entry.transaction_index,
                    entry.prioritization_fee,
                    entry.cu_limit,
                    entry.tx_signature,
                    entry.is_vote
                );
            }
        }
    }

    fn calculate_priority_fee_in_lamports(priority_details: &TransactionPriorityDetails) -> u64 {
        const MICRO_LAMPORTS_PER_LAMPORT: u64 = 1_000_000;
        let micro_lamport_fee = (priority_details.priority as u128)
            .saturating_mul(priority_details.compute_unit_limit as u128);
        let fee = micro_lamport_fee
            .saturating_add(MICRO_LAMPORTS_PER_LAMPORT.saturating_sub(1) as u128)
            .saturating_div(MICRO_LAMPORTS_PER_LAMPORT as u128);
        u64::try_from(fee).unwrap_or(u64::MAX)
    }

    fn scan_market(market: &PriorityFeeMarket) -> Result<(), PrioritizationFeeMarketAnomalies> {
        let pfee = market.front().unwrap().prioritization_fee;
        for entry in market.iter() {
            if pfee < entry.prioritization_fee {
                return Err(PrioritizationFeeMarketAnomalies::HigherPrioritiedTxIsBehind);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        solana_sdk::{
            compute_budget::ComputeBudgetInstruction,
            message::Message,
            pubkey::Pubkey,
            system_instruction,
            transaction::{SanitizedTransaction, Transaction},
        },
    };

    fn build_sanitized_transaction_for_test(
        compute_unit_price: u64,
        signer_account: &Pubkey,
        write_account: &Pubkey,
    ) -> SanitizedTransaction {
        let transaction = Transaction::new_unsigned(Message::new(
            &[
                system_instruction::transfer(signer_account, write_account, 1),
                ComputeBudgetInstruction::set_compute_unit_price(compute_unit_price),
            ],
            Some(signer_account),
        ));

        SanitizedTransaction::try_from_legacy_transaction(transaction).unwrap()
    }

    #[test]
    fn test_priority_fee_by_slot_construct() {
        let slot = 101;
        let mut testee = PriorityFeeBySlot::new(slot);
        assert_eq!(testee.local_markets.len(), 1);
        assert!(testee.local_markets.contains_key(&BLOCK_FEE_MARKET_KEY));

        let signer_key = Pubkey::new_unique();
        let write1_key = Pubkey::new_unique();
        let write2_key = Pubkey::new_unique();
        {
            let tx = build_sanitized_transaction_for_test(100, &signer_key, &write1_key);
            testee.insert(1, 2, &tx);
        }
        {
            let tx = build_sanitized_transaction_for_test(10, &signer_key, &write2_key);
            testee.insert(1, 3, &tx);
        }
        {
            let tx = build_sanitized_transaction_for_test(110, &signer_key, &write1_key);
            testee.insert(2, 1, &tx);
        }

        assert_eq!(testee.local_markets.len(), 4);
        assert!(testee.local_markets.contains_key(&BLOCK_FEE_MARKET_KEY));
        assert!(testee.local_markets.contains_key(&signer_key));
        assert!(testee.local_markets.contains_key(&write1_key));
        assert!(testee.local_markets.contains_key(&write2_key));
        assert_eq!(
            testee
                .local_markets
                .get(&BLOCK_FEE_MARKET_KEY)
                .unwrap()
                .iter()
                .map(|e| e.prioritization_fee)
                .collect::<Vec<_>>(),
            vec![100, 10, 110]
        );
        assert_eq!(
            testee
                .local_markets
                .get(&signer_key)
                .unwrap()
                .iter()
                .map(|e| e.prioritization_fee)
                .collect::<Vec<_>>(),
            vec![100, 10, 110]
        );
        assert_eq!(
            testee
                .local_markets
                .get(&write1_key)
                .unwrap()
                .iter()
                .map(|e| e.prioritization_fee)
                .collect::<Vec<_>>(),
            vec![100, 110]
        );
        assert_eq!(
            testee
                .local_markets
                .get(&write2_key)
                .unwrap()
                .iter()
                .map(|e| e.prioritization_fee)
                .collect::<Vec<_>>(),
            vec![10]
        );

        let anomaly_markets = self
            .local_markets
            .iter()
            .filter_map(|(key, market)| {
                if PriorityFeeBySlot::scan_market(market).is_err() {
                    return Some(*key);
                }
                None
            })
            .collect::<Vec<_>>();
        let number_of_anomaly_local_markets = anomaly_markets.len();

        assert_eq!(self.summary.number_of_transactions, 3);
        assert_eq!(self.summary.number_of_non_vote_transactions, 3);
        assert_eq!(self.summary.number_of_prioritized_transactions, 3);
        assert_eq!(self.summary.total_prioritization_fee, 10);
        assert_eq!(number_of_anomaly_local_markets, 3);
    }
}
