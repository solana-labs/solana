use solana_sdk::{account::Account, pubkey::Pubkey, system_program};

pub(crate) fn create_genesis_accounts(
    mint_pubkey: &Pubkey,
    mint_lamports: u64,
) -> Vec<(Pubkey, Account)> {
    vec![
        // the mint
        (
            *mint_pubkey,
            Account::new(mint_lamports, 0, &system_program::id()),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_genesis_accounts() {
        let mint_lamports = 42;
        let accounts = create_genesis_accounts(&Pubkey::default(), mint_lamports);
        let genesis_lamports: u64 = accounts.iter().map(|(_, account)| account.lamports).sum();
        assert_eq!(genesis_lamports, mint_lamports);
    }
}
