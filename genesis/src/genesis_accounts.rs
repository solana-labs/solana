use hex_literal::hex;
use solana_sdk::{account::Account, pubkey::Pubkey, system_program};

pub(crate) fn create_genesis_accounts(
    mint_pubkey: &Pubkey,
    mint_lamports: u64,
) -> Vec<(Pubkey, Account)> {
    let pubkeys = vec![
        hex!("ab22196afde08a090a3721eb20e3e1ea84d36e14d1a3f0815b236b300d9d33ef"),
        hex!("a2a7ae9098f862f4b3ba7d102d174de5e84a560444c39c035f3eeecce442eadc"),
        hex!("6a56514c29f6b1de4d46164621d6bd25b337a711f569f9283c1143c7e8fb546e"),
        hex!("b420af728f58d9f269d6e07fbbaecf6ed6535e5348538e3f39f2710351f2b940"),
        hex!("ddf2e4c81eafae2d68ac99171b066c87bddb168d6b7c07333cd951f36640163d"),
        hex!("312fa06ccf1b671b26404a34136161ed2aba9e66f248441b4fddb5c592fde560"),
        hex!("0cbf98cd35ceff84ca72b752c32cc3eeee4f765ca1bef1140927ebf5c6e74339"),
        hex!("467e06fa25a9e06824eedc926ce431947ed99c728bed36be54561354c1330959"),
        hex!("ef1562bf9edfd0f5e62530cce4244e8de544a3a30075a2cd5c9074edfbcbe78a"),
        hex!("2ab26abb9d8131a30a4a63446125cf961ece4b926c31cce0eb84da4eac3f836e"),
    ];
    let mut accounts: Vec<_> = pubkeys
        .iter()
        .map(|pubkey_bytes| {
            (
                Pubkey::new(pubkey_bytes),
                Account::new(1_000, 0, &system_program::id()),
            )
        })
        .collect();

    let mut all_accounts = vec![
        // the mint
        (
            *mint_pubkey,
            Account::new(mint_lamports, 0, &system_program::id()),
        ),
    ];
    all_accounts.append(&mut accounts);
    all_accounts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_genesis_accounts() {
        let mint_lamports = 42;
        let accounts = create_genesis_accounts(&Pubkey::default(), mint_lamports);
        let genesis_lamports: u64 = accounts.iter().map(|(_, account)| account.lamports).sum();
        assert_eq!(genesis_lamports, mint_lamports + 10 * 1_000);
    }
}
