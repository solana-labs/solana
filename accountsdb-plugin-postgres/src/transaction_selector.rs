/// The transaction selector is responsible for filtering transactions
/// in the plugin framework.
use {log::*, solana_sdk::pubkey::Pubkey, std::collections::HashSet};

pub(crate) struct TransactionSelector {
    pub mentions: HashSet<Vec<u8>>,
    pub select_all_transactions: bool,
    pub select_all_vote_transactions: bool,
}

impl TransactionSelector {
    pub fn default() -> Self {
        Self {
            mentions: HashSet::default(),
            select_all_transactions: false,
            select_all_vote_transactions: false,
        }
    }

    pub fn new(accounts: &[String]) -> Self {
        info!("Creating TransactionSelector from accounts: {:?}", accounts);

        let select_all_transactions = accounts.iter().any(|key| key == "*" || key == "all");
        if select_all_transactions {
            return Self {
                mentions: HashSet::default(),
                select_all_transactions,
                select_all_vote_transactions: true,
            };
        }
        let select_all_vote_transactions = accounts.iter().any(|key| key == "all_votes");
        if select_all_vote_transactions {
            return Self {
                mentions: HashSet::default(),
                select_all_transactions,
                select_all_vote_transactions: true,
            };
        }

        let accounts = accounts
            .iter()
            .map(|key| bs58::decode(key).into_vec().unwrap())
            .collect();

        Self {
            mentions: accounts,
            select_all_transactions: false,
            select_all_vote_transactions: false,
        }
    }

    pub fn is_transaction_selected(
        &self,
        is_vote: bool,
        accounts: Box<dyn Iterator<Item = &Pubkey> + '_>,
    ) -> bool {
        if self.select_all_transactions || (self.select_all_vote_transactions && is_vote) {
            return true;
        }
        for account in accounts {
            if self.mentions.contains(account.as_ref()) {
                return true;
            }
        }
        false
    }
}
