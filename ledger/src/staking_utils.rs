#[cfg(test)]
pub(crate) mod tests {
    use {
        rand::Rng,
        solana_runtime::bank::Bank,
        solana_sdk::{
            account::AccountSharedData,
            clock::Clock,
            instruction::Instruction,
            pubkey::Pubkey,
            signature::{Keypair, Signer},
            signers::Signers,
            stake::{
                instruction as stake_instruction,
                state::{Authorized, Lockup},
            },
            transaction::Transaction,
        },
        solana_vote::vote_account::{VoteAccount, VoteAccounts},
        solana_vote_program::{
            vote_instruction,
            vote_state::{VoteInit, VoteState, VoteStateVersions},
        },
    };

    pub(crate) fn setup_vote_and_stake_accounts(
        bank: &Bank,
        from_account: &Keypair,
        vote_account: &Keypair,
        validator_identity_account: &Keypair,
        amount: u64,
    ) {
        let vote_pubkey = vote_account.pubkey();
        fn process_instructions<T: Signers>(bank: &Bank, keypairs: &T, ixs: &[Instruction]) {
            let tx = Transaction::new_signed_with_payer(
                ixs,
                Some(&keypairs.pubkeys()[0]),
                keypairs,
                bank.last_blockhash(),
            );
            bank.process_transaction(&tx).unwrap();
        }

        process_instructions(
            bank,
            &[from_account, vote_account, validator_identity_account],
            &vote_instruction::create_account_with_config(
                &from_account.pubkey(),
                &vote_pubkey,
                &VoteInit {
                    node_pubkey: validator_identity_account.pubkey(),
                    authorized_voter: vote_pubkey,
                    authorized_withdrawer: vote_pubkey,
                    commission: 0,
                },
                amount,
                vote_instruction::CreateVoteAccountConfig {
                    space: VoteStateVersions::vote_state_size_of(true) as u64,
                    ..vote_instruction::CreateVoteAccountConfig::default()
                },
            ),
        );

        let stake_account_keypair = Keypair::new();
        let stake_account_pubkey = stake_account_keypair.pubkey();

        process_instructions(
            bank,
            &[from_account, &stake_account_keypair],
            &stake_instruction::create_account_and_delegate_stake(
                &from_account.pubkey(),
                &stake_account_pubkey,
                &vote_pubkey,
                &Authorized::auto(&stake_account_pubkey),
                &Lockup::default(),
                amount,
            ),
        );
    }

    #[test]
    fn test_to_staked_nodes() {
        let mut stakes = Vec::new();
        let node1 = solana_sdk::pubkey::new_rand();

        // Node 1 has stake of 3
        for i in 0..3 {
            stakes.push((
                i,
                VoteState::new(
                    &VoteInit {
                        node_pubkey: node1,
                        ..VoteInit::default()
                    },
                    &Clock::default(),
                ),
            ));
        }

        // Node 1 has stake of 5
        let node2 = solana_sdk::pubkey::new_rand();

        stakes.push((
            5,
            VoteState::new(
                &VoteInit {
                    node_pubkey: node2,
                    ..VoteInit::default()
                },
                &Clock::default(),
            ),
        ));
        let mut rng = rand::thread_rng();
        let vote_accounts = stakes.into_iter().map(|(stake, vote_state)| {
            let account = AccountSharedData::new_data(
                rng.gen(), // lamports
                &VoteStateVersions::new_current(vote_state),
                &solana_vote_program::id(), // owner
            )
            .unwrap();
            let vote_pubkey = Pubkey::new_unique();
            let vote_account = VoteAccount::try_from(account).unwrap();
            (vote_pubkey, (stake, vote_account))
        });
        let result = vote_accounts.collect::<VoteAccounts>().staked_nodes();
        assert_eq!(result.len(), 2);
        assert_eq!(result[&node1], 3);
        assert_eq!(result[&node2], 5);
    }
}
