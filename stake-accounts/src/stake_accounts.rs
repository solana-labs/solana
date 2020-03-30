use solana_sdk::{instruction::Instruction, message::Message, pubkey::Pubkey};
use solana_stake_program::{
    stake_instruction,
    stake_state::{Authorized, Lockup, StakeAuthorize},
};

pub(crate) fn derive_stake_account_address(base_pubkey: &Pubkey, i: usize) -> Pubkey {
    Pubkey::create_with_seed(base_pubkey, &i.to_string(), &solana_stake_program::id()).unwrap()
}

// Return derived addresses
pub(crate) fn derive_stake_account_addresses(
    base_pubkey: &Pubkey,
    num_accounts: usize,
) -> Vec<Pubkey> {
    (0..num_accounts)
        .map(|i| derive_stake_account_address(base_pubkey, i))
        .collect()
}

pub(crate) fn new_stake_account(
    fee_payer_pubkey: &Pubkey,
    funding_pubkey: &Pubkey,
    base_pubkey: &Pubkey,
    lamports: u64,
    stake_authority_pubkey: &Pubkey,
    withdraw_authority_pubkey: &Pubkey,
    index: usize,
) -> Message {
    let stake_account_address = derive_stake_account_address(base_pubkey, index);
    let authorized = Authorized {
        staker: *stake_authority_pubkey,
        withdrawer: *withdraw_authority_pubkey,
    };
    let instructions = stake_instruction::create_account_with_seed(
        funding_pubkey,
        &stake_account_address,
        &base_pubkey,
        &index.to_string(),
        &authorized,
        &Lockup::default(),
        lamports,
    );
    Message::new_with_payer(&instructions, Some(fee_payer_pubkey))
}

fn authorize_stake_accounts_instructions(
    stake_account_address: &Pubkey,
    stake_authority_pubkey: &Pubkey,
    withdraw_authority_pubkey: &Pubkey,
    new_stake_authority_pubkey: &Pubkey,
    new_withdraw_authority_pubkey: &Pubkey,
) -> Vec<Instruction> {
    let instruction0 = stake_instruction::authorize(
        &stake_account_address,
        stake_authority_pubkey,
        new_stake_authority_pubkey,
        StakeAuthorize::Staker,
    );
    let instruction1 = stake_instruction::authorize(
        &stake_account_address,
        withdraw_authority_pubkey,
        new_withdraw_authority_pubkey,
        StakeAuthorize::Withdrawer,
    );
    vec![instruction0, instruction1]
}

fn rebase_stake_account(
    stake_account_address: &Pubkey,
    new_base_pubkey: &Pubkey,
    i: usize,
    fee_payer_pubkey: &Pubkey,
    stake_authority_pubkey: &Pubkey,
    lamports: u64,
) -> Message {
    let new_stake_account_address = derive_stake_account_address(new_base_pubkey, i);
    let instructions = stake_instruction::split_with_seed(
        stake_account_address,
        stake_authority_pubkey,
        lamports,
        &new_stake_account_address,
        new_base_pubkey,
        &i.to_string(),
    );
    Message::new_with_payer(&instructions, Some(&fee_payer_pubkey))
}

fn move_stake_account(
    stake_account_address: &Pubkey,
    new_base_pubkey: &Pubkey,
    i: usize,
    fee_payer_pubkey: &Pubkey,
    stake_authority_pubkey: &Pubkey,
    withdraw_authority_pubkey: &Pubkey,
    new_stake_authority_pubkey: &Pubkey,
    new_withdraw_authority_pubkey: &Pubkey,
    lamports: u64,
) -> Message {
    let new_stake_account_address = derive_stake_account_address(new_base_pubkey, i);
    let mut instructions = stake_instruction::split_with_seed(
        stake_account_address,
        stake_authority_pubkey,
        lamports,
        &new_stake_account_address,
        new_base_pubkey,
        &i.to_string(),
    );

    let authorize_instructions = authorize_stake_accounts_instructions(
        &new_stake_account_address,
        stake_authority_pubkey,
        withdraw_authority_pubkey,
        new_stake_authority_pubkey,
        new_withdraw_authority_pubkey,
    );

    instructions.extend(authorize_instructions.into_iter());
    Message::new_with_payer(&instructions, Some(&fee_payer_pubkey))
}

pub(crate) fn authorize_stake_accounts(
    fee_payer_pubkey: &Pubkey,
    base_pubkey: &Pubkey,
    stake_authority_pubkey: &Pubkey,
    withdraw_authority_pubkey: &Pubkey,
    new_stake_authority_pubkey: &Pubkey,
    new_withdraw_authority_pubkey: &Pubkey,
    num_accounts: usize,
) -> Vec<Message> {
    let stake_account_addresses = derive_stake_account_addresses(base_pubkey, num_accounts);
    stake_account_addresses
        .iter()
        .map(|stake_account_address| {
            let instructions = authorize_stake_accounts_instructions(
                stake_account_address,
                stake_authority_pubkey,
                withdraw_authority_pubkey,
                new_stake_authority_pubkey,
                new_withdraw_authority_pubkey,
            );
            Message::new_with_payer(&instructions, Some(&fee_payer_pubkey))
        })
        .collect::<Vec<_>>()
}

pub(crate) fn rebase_stake_accounts(
    fee_payer_pubkey: &Pubkey,
    new_base_pubkey: &Pubkey,
    stake_authority_pubkey: &Pubkey,
    balances: &[(Pubkey, u64)],
) -> Vec<Message> {
    balances
        .iter()
        .enumerate()
        .map(|(i, (stake_account_address, lamports))| {
            rebase_stake_account(
                stake_account_address,
                new_base_pubkey,
                i,
                fee_payer_pubkey,
                stake_authority_pubkey,
                *lamports,
            )
        })
        .collect()
}

pub(crate) fn move_stake_accounts(
    fee_payer_pubkey: &Pubkey,
    new_base_pubkey: &Pubkey,
    stake_authority_pubkey: &Pubkey,
    withdraw_authority_pubkey: &Pubkey,
    new_stake_authority_pubkey: &Pubkey,
    new_withdraw_authority_pubkey: &Pubkey,
    balances: &[(Pubkey, u64)],
) -> Vec<Message> {
    balances
        .iter()
        .enumerate()
        .map(|(i, (stake_account_address, lamports))| {
            move_stake_account(
                stake_account_address,
                new_base_pubkey,
                i,
                fee_payer_pubkey,
                stake_authority_pubkey,
                withdraw_authority_pubkey,
                new_stake_authority_pubkey,
                new_withdraw_authority_pubkey,
                *lamports,
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_runtime::{bank::Bank, bank_client::BankClient};
    use solana_sdk::{
        account::Account,
        client::SyncClient,
        genesis_config::create_genesis_config,
        signature::{Keypair, Signer},
    };
    use solana_stake_program::stake_state::StakeState;

    fn create_bank(lamports: u64) -> (Bank, Keypair, u64) {
        let (genesis_config, mint_keypair) = create_genesis_config(lamports);
        let mut bank = Bank::new(&genesis_config);
        bank.add_instruction_processor(
            solana_stake_program::id(),
            solana_stake_program::stake_instruction::process_instruction,
        );
        let rent = bank.get_minimum_balance_for_rent_exemption(std::mem::size_of::<StakeState>());
        (bank, mint_keypair, rent)
    }

    fn create_account<C: SyncClient>(
        client: &C,
        funding_keypair: &Keypair,
        lamports: u64,
    ) -> Keypair {
        let fee_payer_keypair = Keypair::new();
        client
            .transfer(lamports, &funding_keypair, &fee_payer_keypair.pubkey())
            .unwrap();
        fee_payer_keypair
    }

    fn get_account_at<C: SyncClient>(client: &C, base_pubkey: &Pubkey, i: usize) -> Account {
        let account_address = derive_stake_account_address(&base_pubkey, i);
        client.get_account(&account_address).unwrap().unwrap()
    }

    fn get_balances<C: SyncClient>(
        client: &C,
        base_pubkey: &Pubkey,
        num_accounts: usize,
    ) -> Vec<(Pubkey, u64)> {
        (0..num_accounts)
            .into_iter()
            .map(|i| {
                let address = derive_stake_account_address(&base_pubkey, i);
                (address, client.get_balance(&address).unwrap())
            })
            .collect()
    }

    #[test]
    fn test_new_derived_stake_account() {
        let (bank, funding_keypair, rent) = create_bank(10_000_000);
        let funding_pubkey = funding_keypair.pubkey();
        let bank_client = BankClient::new(bank);
        let fee_payer_keypair = create_account(&bank_client, &funding_keypair, 1);
        let fee_payer_pubkey = fee_payer_keypair.pubkey();

        let base_keypair = Keypair::new();
        let base_pubkey = base_keypair.pubkey();
        let lamports = rent + 1;
        let stake_authority_pubkey = Pubkey::new_rand();
        let withdraw_authority_pubkey = Pubkey::new_rand();

        let message = new_stake_account(
            &fee_payer_pubkey,
            &funding_pubkey,
            &base_pubkey,
            lamports,
            &stake_authority_pubkey,
            &withdraw_authority_pubkey,
            0,
        );

        let signers = [&funding_keypair, &fee_payer_keypair, &base_keypair];
        bank_client.send_message(&signers, message).unwrap();

        let account = get_account_at(&bank_client, &base_pubkey, 0);
        assert_eq!(account.lamports, lamports);
        let authorized = StakeState::authorized_from(&account).unwrap();
        assert_eq!(authorized.staker, stake_authority_pubkey);
        assert_eq!(authorized.withdrawer, withdraw_authority_pubkey);
    }

    #[test]
    fn test_authorize_stake_accounts() {
        let (bank, funding_keypair, rent) = create_bank(10_000_000);
        let funding_pubkey = funding_keypair.pubkey();
        let bank_client = BankClient::new(bank);
        let fee_payer_keypair = create_account(&bank_client, &funding_keypair, 1);
        let fee_payer_pubkey = fee_payer_keypair.pubkey();

        let base_keypair = Keypair::new();
        let base_pubkey = base_keypair.pubkey();
        let lamports = rent + 1;

        let stake_authority_keypair = Keypair::new();
        let stake_authority_pubkey = stake_authority_keypair.pubkey();
        let withdraw_authority_keypair = Keypair::new();
        let withdraw_authority_pubkey = withdraw_authority_keypair.pubkey();

        let message = new_stake_account(
            &fee_payer_pubkey,
            &funding_pubkey,
            &base_pubkey,
            lamports,
            &stake_authority_pubkey,
            &withdraw_authority_pubkey,
            0,
        );

        let signers = [&funding_keypair, &fee_payer_keypair, &base_keypair];
        bank_client.send_message(&signers, message).unwrap();

        let new_stake_authority_pubkey = Pubkey::new_rand();
        let new_withdraw_authority_pubkey = Pubkey::new_rand();
        let messages = authorize_stake_accounts(
            &fee_payer_pubkey,
            &base_pubkey,
            &stake_authority_pubkey,
            &withdraw_authority_pubkey,
            &new_stake_authority_pubkey,
            &new_withdraw_authority_pubkey,
            1,
        );

        let signers = [
            &fee_payer_keypair,
            &stake_authority_keypair,
            &withdraw_authority_keypair,
        ];
        for message in messages {
            bank_client.send_message(&signers, message).unwrap();
        }

        let account = get_account_at(&bank_client, &base_pubkey, 0);
        let authorized = StakeState::authorized_from(&account).unwrap();
        assert_eq!(authorized.staker, new_stake_authority_pubkey);
        assert_eq!(authorized.withdrawer, new_withdraw_authority_pubkey);
    }

    #[test]
    fn test_rebase_stake_accounts() {
        let (bank, funding_keypair, rent) = create_bank(10_000_000);
        let funding_pubkey = funding_keypair.pubkey();
        let bank_client = BankClient::new(bank);
        let fee_payer_keypair = create_account(&bank_client, &funding_keypair, 1);
        let fee_payer_pubkey = fee_payer_keypair.pubkey();

        let base_keypair = Keypair::new();
        let base_pubkey = base_keypair.pubkey();
        let lamports = rent + 1;

        let stake_authority_keypair = Keypair::new();
        let stake_authority_pubkey = stake_authority_keypair.pubkey();
        let withdraw_authority_keypair = Keypair::new();
        let withdraw_authority_pubkey = withdraw_authority_keypair.pubkey();

        let num_accounts = 1;
        let message = new_stake_account(
            &fee_payer_pubkey,
            &funding_pubkey,
            &base_pubkey,
            lamports,
            &stake_authority_pubkey,
            &withdraw_authority_pubkey,
            0,
        );

        let signers = [&funding_keypair, &fee_payer_keypair, &base_keypair];
        bank_client.send_message(&signers, message).unwrap();

        let new_base_keypair = Keypair::new();
        let new_base_pubkey = new_base_keypair.pubkey();
        let balances = get_balances(&bank_client, &base_pubkey, num_accounts);
        let messages = rebase_stake_accounts(
            &fee_payer_pubkey,
            &new_base_pubkey,
            &stake_authority_pubkey,
            &balances,
        );
        assert_eq!(messages.len(), num_accounts);

        let signers = [
            &fee_payer_keypair,
            &new_base_keypair,
            &stake_authority_keypair,
        ];
        for message in messages {
            bank_client.send_message(&signers, message).unwrap();
        }

        // Ensure the new accounts are duplicates of the previous ones.
        let account = get_account_at(&bank_client, &new_base_pubkey, 0);
        let authorized = StakeState::authorized_from(&account).unwrap();
        assert_eq!(authorized.staker, stake_authority_pubkey);
        assert_eq!(authorized.withdrawer, withdraw_authority_pubkey);
    }

    #[test]
    fn test_move_stake_accounts() {
        let (bank, funding_keypair, rent) = create_bank(10_000_000);
        let funding_pubkey = funding_keypair.pubkey();
        let bank_client = BankClient::new(bank);
        let fee_payer_keypair = create_account(&bank_client, &funding_keypair, 1);
        let fee_payer_pubkey = fee_payer_keypair.pubkey();

        let base_keypair = Keypair::new();
        let base_pubkey = base_keypair.pubkey();
        let lamports = rent + 1;

        let stake_authority_keypair = Keypair::new();
        let stake_authority_pubkey = stake_authority_keypair.pubkey();
        let withdraw_authority_keypair = Keypair::new();
        let withdraw_authority_pubkey = withdraw_authority_keypair.pubkey();

        let num_accounts = 1;
        let message = new_stake_account(
            &fee_payer_pubkey,
            &funding_pubkey,
            &base_pubkey,
            lamports,
            &stake_authority_pubkey,
            &withdraw_authority_pubkey,
            0,
        );

        let signers = [&funding_keypair, &fee_payer_keypair, &base_keypair];
        bank_client.send_message(&signers, message).unwrap();

        let new_base_keypair = Keypair::new();
        let new_base_pubkey = new_base_keypair.pubkey();
        let new_stake_authority_pubkey = Pubkey::new_rand();
        let new_withdraw_authority_pubkey = Pubkey::new_rand();
        let balances = get_balances(&bank_client, &base_pubkey, num_accounts);
        let messages = move_stake_accounts(
            &fee_payer_pubkey,
            &new_base_pubkey,
            &stake_authority_pubkey,
            &withdraw_authority_pubkey,
            &new_stake_authority_pubkey,
            &new_withdraw_authority_pubkey,
            &balances,
        );
        assert_eq!(messages.len(), num_accounts);

        let signers = [
            &fee_payer_keypair,
            &new_base_keypair,
            &stake_authority_keypair,
            &withdraw_authority_keypair,
        ];
        for message in messages {
            bank_client.send_message(&signers, message).unwrap();
        }

        // Ensure the new accounts have the new authorities.
        let account = get_account_at(&bank_client, &new_base_pubkey, 0);
        let authorized = StakeState::authorized_from(&account).unwrap();
        assert_eq!(authorized.staker, new_stake_authority_pubkey);
        assert_eq!(authorized.withdrawer, new_withdraw_authority_pubkey);
    }
}
