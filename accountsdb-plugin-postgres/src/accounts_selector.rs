use {log::*, std::collections::HashSet};

#[derive(Debug)]
pub(crate) struct AccountsSelector {
    pub accounts: HashSet<String>,
    pub owners: HashSet<String>,
    pub select_all_accounts: bool,
}

impl AccountsSelector {
    pub fn default() -> Self {
        AccountsSelector {
            accounts: HashSet::default(),
            owners: HashSet::default(),
            select_all_accounts: true,
        }
    }

    pub fn new(accounts: &[String], owners: &[String]) -> Self {
        info!(
            "Creating AccountsSelector from accounts: {:?}, owners: {:?}",
            accounts, owners
        );

        let select_all_accounts = accounts.iter().any(|key| key == "*");
        if select_all_accounts {
            return AccountsSelector {
                accounts: HashSet::default(),
                owners: HashSet::default(),
                select_all_accounts,
            };
        }
        let accounts = accounts.iter().cloned().collect();
        let owners = owners.iter().cloned().collect();
        AccountsSelector {
            accounts,
            owners,
            select_all_accounts,
        }
    }

    pub fn is_account_selected(&self, account: &str, owner: &str) -> bool {
        self.select_all_accounts || self.accounts.contains(account) || self.owners.contains(owner)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn test_create_accounts_selector() {
        AccountsSelector::new(
            &["9xQeWvG816bUx9EPjHmaT23yvVM2ZWbrrpZb9PusVFin".to_string()],
            &[],
        );

        AccountsSelector::new(
            &[],
            &["9xQeWvG816bUx9EPjHmaT23yvVM2ZWbrrpZb9PusVFin".to_string()],
        );
    }
}
