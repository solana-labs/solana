use solana_sdk::{account::Account, pubkey::Pubkey};

pub(crate) fn create_genesis_accounts() -> Vec<(Pubkey, Account)> {
    vec![]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_genesis_accounts() {
        assert_eq!(create_genesis_accounts(), vec![]);
    }
}
