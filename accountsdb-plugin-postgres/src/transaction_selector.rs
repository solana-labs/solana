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

    /// Check if a transaction is of interest.
    pub fn is_transaction_selected(
        &self,
        is_vote: bool,
        accounts: Box<dyn Iterator<Item = &Pubkey> + '_>,
    ) -> bool {
        if !self.is_interested_in_any_transaction() {
            return false;
        }

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

    /// Check if any transaction is of interested at all
    pub fn is_interested_in_any_transaction(&self) -> bool {
        self.select_all_transactions
            || self.select_all_vote_transactions
            || !self.mentions.is_empty()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn test_select_transaction() {
        let pubkey1 = Pubkey::new_unique();
        let pubkey2 = Pubkey::new_unique();

        let selector = TransactionSelector::new(&[pubkey1.to_string()]);

        assert!(selector.is_interested_in_any_transaction());

        let accounts = [pubkey1];

        assert!(selector.is_transaction_selected(false, Box::new(accounts.iter())));

        let accounts = [pubkey2];
        assert!(!selector.is_transaction_selected(false, Box::new(accounts.iter())));

        let accounts = [pubkey1, pubkey2];
        assert!(selector.is_transaction_selected(false, Box::new(accounts.iter())));
    }

    #[test]
    fn test_select_all_transaction_using_wildcard() {
        let pubkey1 = Pubkey::new_unique();
        let pubkey2 = Pubkey::new_unique();

        let selector = TransactionSelector::new(&["*".to_string()]);

        assert!(selector.is_interested_in_any_transaction());

        let accounts = [pubkey1];

        assert!(selector.is_transaction_selected(false, Box::new(accounts.iter())));

        let accounts = [pubkey2];
        assert!(selector.is_transaction_selected(false, Box::new(accounts.iter())));

        let accounts = [pubkey1, pubkey2];
        assert!(selector.is_transaction_selected(false, Box::new(accounts.iter())));
    }

    #[test]
    fn test_select_all_transaction_all() {
        let pubkey1 = Pubkey::new_unique();
        let pubkey2 = Pubkey::new_unique();

        let selector = TransactionSelector::new(&["all".to_string()]);

        assert!(selector.is_interested_in_any_transaction());

        let accounts = [pubkey1];

        assert!(selector.is_transaction_selected(false, Box::new(accounts.iter())));

        let accounts = [pubkey2];
        assert!(selector.is_transaction_selected(false, Box::new(accounts.iter())));

        let accounts = [pubkey1, pubkey2];
        assert!(selector.is_transaction_selected(false, Box::new(accounts.iter())));
    }

    #[test]
    fn test_select_all_vote_transaction() {
        let pubkey1 = Pubkey::new_unique();
        let pubkey2 = Pubkey::new_unique();

        let selector = TransactionSelector::new(&["all_votes".to_string()]);

        assert!(selector.is_interested_in_any_transaction());

        let accounts = [pubkey1];

        assert!(!selector.is_transaction_selected(false, Box::new(accounts.iter())));

        let accounts = [pubkey2];
        assert!(selector.is_transaction_selected(true, Box::new(accounts.iter())));

        let accounts = [pubkey1, pubkey2];
        assert!(selector.is_transaction_selected(true, Box::new(accounts.iter())));
    }

    #[test]
    fn test_select_no_transaction() {
        let pubkey1 = Pubkey::new_unique();
        let pubkey2 = Pubkey::new_unique();

        let selector = TransactionSelector::new(&[]);

        assert!(!selector.is_interested_in_any_transaction());

        let accounts = [pubkey1];

        assert!(!selector.is_transaction_selected(false, Box::new(accounts.iter())));

        let accounts = [pubkey2];
        assert!(!selector.is_transaction_selected(true, Box::new(accounts.iter())));

        let accounts = [pubkey1, pubkey2];
        assert!(!selector.is_transaction_selected(true, Box::new(accounts.iter())));
    }
}
