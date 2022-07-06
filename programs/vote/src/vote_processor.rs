//! Vote program processor

use {
    crate::{id, vote_instruction::VoteInstruction, vote_state},
    log::*,
    solana_metrics::inc_new_counter_info,
    solana_program_runtime::{
        invoke_context::InvokeContext, sysvar_cache::get_sysvar_with_account_check,
    },
    solana_sdk::{
        feature_set,
        instruction::InstructionError,
        keyed_account::{get_signers, keyed_account_at_index, KeyedAccount},
        program_utils::limited_deserialize,
        pubkey::Pubkey,
        sysvar::rent::Rent,
    },
    std::collections::HashSet,
};

pub fn process_instruction(
    first_instruction_account: usize,
    data: &[u8],
    invoke_context: &mut InvokeContext,
) -> Result<(), InstructionError> {
    let keyed_accounts = invoke_context.get_keyed_accounts()?;

    trace!("process_instruction: {:?}", data);
    trace!("keyed_accounts: {:?}", keyed_accounts);

    let me = &mut keyed_account_at_index(keyed_accounts, first_instruction_account)?;
    if me.owner()? != id() {
        return Err(InstructionError::InvalidAccountOwner);
    }

    let signers: HashSet<Pubkey> = get_signers(&keyed_accounts[first_instruction_account..]);
    match limited_deserialize(data)? {
        VoteInstruction::InitializeAccount(vote_init) => {
            let rent = get_sysvar_with_account_check::rent(
                keyed_account_at_index(keyed_accounts, first_instruction_account + 1)?,
                invoke_context,
            )?;
            verify_rent_exemption(me, &rent)?;
            let clock = get_sysvar_with_account_check::clock(
                keyed_account_at_index(keyed_accounts, first_instruction_account + 2)?,
                invoke_context,
            )?;
            vote_state::initialize_account(me, &vote_init, &signers, &clock)
        }
        VoteInstruction::Authorize(voter_pubkey, vote_authorize) => {
            let clock = get_sysvar_with_account_check::clock(
                keyed_account_at_index(keyed_accounts, first_instruction_account + 1)?,
                invoke_context,
            )?;
            vote_state::authorize(
                me,
                &voter_pubkey,
                vote_authorize,
                &signers,
                &clock,
                &invoke_context.feature_set,
            )
        }
        VoteInstruction::UpdateValidatorIdentity => vote_state::update_validator_identity(
            me,
            keyed_account_at_index(keyed_accounts, first_instruction_account + 1)?.unsigned_key(),
            &signers,
        ),
        VoteInstruction::UpdateCommission(commission) => {
            vote_state::update_commission(me, commission, &signers)
        }
        VoteInstruction::Vote(vote) | VoteInstruction::VoteSwitch(vote, _) => {
            inc_new_counter_info!("vote-native", 1);
            let slot_hashes = get_sysvar_with_account_check::slot_hashes(
                keyed_account_at_index(keyed_accounts, first_instruction_account + 1)?,
                invoke_context,
            )?;
            let clock = get_sysvar_with_account_check::clock(
                keyed_account_at_index(keyed_accounts, first_instruction_account + 2)?,
                invoke_context,
            )?;
            vote_state::process_vote(
                me,
                &slot_hashes,
                &clock,
                &vote,
                &signers,
                &invoke_context.feature_set,
            )
        }
        VoteInstruction::UpdateVoteState(vote_state_update)
        | VoteInstruction::UpdateVoteStateSwitch(vote_state_update, _) => {
            if invoke_context
                .feature_set
                .is_active(&feature_set::allow_votes_to_directly_update_vote_state::id())
            {
                inc_new_counter_info!("vote-state-native", 1);
                let sysvar_cache = invoke_context.get_sysvar_cache();
                let slot_hashes = sysvar_cache.get_slot_hashes()?;
                let clock = sysvar_cache.get_clock()?;
                vote_state::process_vote_state_update(
                    me,
                    slot_hashes.slot_hashes(),
                    &clock,
                    vote_state_update,
                    &signers,
                )
            } else {
                Err(InstructionError::InvalidInstructionData)
            }
        }
        VoteInstruction::Withdraw(lamports) => {
            let to = keyed_account_at_index(keyed_accounts, first_instruction_account + 1)?;
            let rent_sysvar = if invoke_context
                .feature_set
                .is_active(&feature_set::reject_non_rent_exempt_vote_withdraws::id())
            {
                Some(invoke_context.get_sysvar_cache().get_rent()?)
            } else {
                None
            };

            let clock_if_feature_active = if invoke_context
                .feature_set
                .is_active(&feature_set::reject_vote_account_close_unless_zero_credit_epoch::id())
            {
                Some(invoke_context.get_sysvar_cache().get_clock()?)
            } else {
                None
            };

            vote_state::withdraw(
                me,
                lamports,
                to,
                &signers,
                rent_sysvar.as_deref(),
                clock_if_feature_active.as_deref(),
            )
        }
        VoteInstruction::AuthorizeChecked(vote_authorize) => {
            if invoke_context
                .feature_set
                .is_active(&feature_set::vote_stake_checked_instructions::id())
            {
                let voter_pubkey =
                    &keyed_account_at_index(keyed_accounts, first_instruction_account + 3)?
                        .signer_key()
                        .ok_or(InstructionError::MissingRequiredSignature)?;
                let clock = get_sysvar_with_account_check::clock(
                    keyed_account_at_index(keyed_accounts, first_instruction_account + 1)?,
                    invoke_context,
                )?;
                vote_state::authorize(
                    me,
                    voter_pubkey,
                    vote_authorize,
                    &signers,
                    &clock,
                    &invoke_context.feature_set,
                )
            } else {
                Err(InstructionError::InvalidInstructionData)
            }
        }
    }
}

fn verify_rent_exemption(
    keyed_account: &KeyedAccount,
    rent: &Rent,
) -> Result<(), InstructionError> {
    if !rent.is_exempt(keyed_account.lamports()?, keyed_account.data_len()?) {
        Err(InstructionError::InsufficientFunds)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{
            vote_instruction::{
                authorize, authorize_checked, create_account, update_commission,
                update_validator_identity, update_vote_state, update_vote_state_switch, vote,
                vote_switch, withdraw, VoteInstruction,
            },
            vote_state::{Vote, VoteAuthorize, VoteInit, VoteState, VoteStateUpdate},
        },
        bincode::serialize,
        solana_program_runtime::invoke_context::mock_process_instruction,
        solana_sdk::{
            account::{self, Account, AccountSharedData},
            hash::Hash,
            instruction::{AccountMeta, Instruction},
            sysvar::{self, clock::Clock, slot_hashes::SlotHashes},
        },
        std::str::FromStr,
    };

    fn create_default_account() -> AccountSharedData {
        AccountSharedData::new(0, 0, &Pubkey::new_unique())
    }

    fn process_instruction(
        instruction_data: &[u8],
        transaction_accounts: Vec<(Pubkey, AccountSharedData)>,
        instruction_accounts: Vec<AccountMeta>,
        expected_result: Result<(), InstructionError>,
    ) -> Vec<AccountSharedData> {
        mock_process_instruction(
            &id(),
            Vec::new(),
            instruction_data,
            transaction_accounts,
            instruction_accounts,
            expected_result,
            super::process_instruction,
        )
    }

    fn process_instruction_as_one_arg(
        instruction: &Instruction,
        expected_result: Result<(), InstructionError>,
    ) -> Vec<AccountSharedData> {
        let mut pubkeys: HashSet<Pubkey> = instruction
            .accounts
            .iter()
            .map(|meta| meta.pubkey)
            .collect();
        pubkeys.insert(sysvar::clock::id());
        pubkeys.insert(sysvar::rent::id());
        pubkeys.insert(sysvar::slot_hashes::id());
        let transaction_accounts: Vec<_> = pubkeys
            .iter()
            .map(|pubkey| {
                (
                    *pubkey,
                    if sysvar::clock::check_id(pubkey) {
                        account::create_account_shared_data_for_test(&Clock::default())
                    } else if sysvar::slot_hashes::check_id(pubkey) {
                        account::create_account_shared_data_for_test(&SlotHashes::default())
                    } else if sysvar::rent::check_id(pubkey) {
                        account::create_account_shared_data_for_test(&Rent::free())
                    } else if *pubkey == invalid_vote_state_pubkey() {
                        AccountSharedData::from(Account {
                            owner: invalid_vote_state_pubkey(),
                            ..Account::default()
                        })
                    } else {
                        AccountSharedData::from(Account {
                            owner: id(),
                            ..Account::default()
                        })
                    },
                )
            })
            .collect();
        process_instruction(
            &instruction.data,
            transaction_accounts,
            instruction.accounts.clone(),
            expected_result,
        )
    }

    fn invalid_vote_state_pubkey() -> Pubkey {
        Pubkey::from_str("BadVote111111111111111111111111111111111111").unwrap()
    }

    // these are for 100% coverage in this file
    #[test]
    fn test_vote_process_instruction_decode_bail() {
        process_instruction(
            &[],
            Vec::new(),
            Vec::new(),
            Err(InstructionError::NotEnoughAccountKeys),
        );
    }

    #[test]
    fn test_spoofed_vote() {
        process_instruction_as_one_arg(
            &vote(
                &invalid_vote_state_pubkey(),
                &Pubkey::new_unique(),
                Vote::default(),
            ),
            Err(InstructionError::InvalidAccountOwner),
        );
        process_instruction_as_one_arg(
            &update_vote_state(
                &invalid_vote_state_pubkey(),
                &Pubkey::default(),
                VoteStateUpdate::default(),
            ),
            Err(InstructionError::InvalidAccountOwner),
        );
    }

    #[test]
    fn test_vote_process_instruction() {
        solana_logger::setup();
        let instructions = create_account(
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            &VoteInit::default(),
            101,
        );
        process_instruction_as_one_arg(&instructions[1], Err(InstructionError::InvalidAccountData));
        process_instruction_as_one_arg(
            &vote(
                &Pubkey::new_unique(),
                &Pubkey::new_unique(),
                Vote::default(),
            ),
            Err(InstructionError::InvalidAccountData),
        );
        process_instruction_as_one_arg(
            &vote_switch(
                &Pubkey::new_unique(),
                &Pubkey::new_unique(),
                Vote::default(),
                Hash::default(),
            ),
            Err(InstructionError::InvalidAccountData),
        );
        process_instruction_as_one_arg(
            &authorize(
                &Pubkey::new_unique(),
                &Pubkey::new_unique(),
                &Pubkey::new_unique(),
                VoteAuthorize::Voter,
            ),
            Err(InstructionError::InvalidAccountData),
        );
        process_instruction_as_one_arg(
            &update_vote_state(
                &Pubkey::default(),
                &Pubkey::default(),
                VoteStateUpdate::default(),
            ),
            Err(InstructionError::InvalidAccountData),
        );

        process_instruction_as_one_arg(
            &update_vote_state_switch(
                &Pubkey::default(),
                &Pubkey::default(),
                VoteStateUpdate::default(),
                Hash::default(),
            ),
            Err(InstructionError::InvalidAccountData),
        );

        process_instruction_as_one_arg(
            &update_validator_identity(
                &Pubkey::new_unique(),
                &Pubkey::new_unique(),
                &Pubkey::new_unique(),
            ),
            Err(InstructionError::InvalidAccountData),
        );
        process_instruction_as_one_arg(
            &update_commission(&Pubkey::new_unique(), &Pubkey::new_unique(), 0),
            Err(InstructionError::InvalidAccountData),
        );

        process_instruction_as_one_arg(
            &withdraw(
                &Pubkey::new_unique(),
                &Pubkey::new_unique(),
                0,
                &Pubkey::new_unique(),
            ),
            Err(InstructionError::InvalidAccountData),
        );
    }

    #[test]
    fn test_vote_authorize_checked() {
        let vote_pubkey = Pubkey::new_unique();
        let authorized_pubkey = Pubkey::new_unique();
        let new_authorized_pubkey = Pubkey::new_unique();

        // Test with vanilla authorize accounts
        let mut instruction = authorize_checked(
            &vote_pubkey,
            &authorized_pubkey,
            &new_authorized_pubkey,
            VoteAuthorize::Voter,
        );
        instruction.accounts = instruction.accounts[0..2].to_vec();
        process_instruction_as_one_arg(&instruction, Err(InstructionError::NotEnoughAccountKeys));

        let mut instruction = authorize_checked(
            &vote_pubkey,
            &authorized_pubkey,
            &new_authorized_pubkey,
            VoteAuthorize::Withdrawer,
        );
        instruction.accounts = instruction.accounts[0..2].to_vec();
        process_instruction_as_one_arg(&instruction, Err(InstructionError::NotEnoughAccountKeys));

        // Test with non-signing new_authorized_pubkey
        let mut instruction = authorize_checked(
            &vote_pubkey,
            &authorized_pubkey,
            &new_authorized_pubkey,
            VoteAuthorize::Voter,
        );
        instruction.accounts[3] = AccountMeta::new_readonly(new_authorized_pubkey, false);
        process_instruction_as_one_arg(
            &instruction,
            Err(InstructionError::MissingRequiredSignature),
        );

        let mut instruction = authorize_checked(
            &vote_pubkey,
            &authorized_pubkey,
            &new_authorized_pubkey,
            VoteAuthorize::Withdrawer,
        );
        instruction.accounts[3] = AccountMeta::new_readonly(new_authorized_pubkey, false);
        process_instruction_as_one_arg(
            &instruction,
            Err(InstructionError::MissingRequiredSignature),
        );

        // Test with new_authorized_pubkey signer
        let vote_account = AccountSharedData::new(100, VoteState::size_of(), &id());
        let clock_address = sysvar::clock::id();
        let clock_account = account::create_account_shared_data_for_test(&Clock::default());
        let default_authorized_pubkey = Pubkey::default();
        let authorized_account = create_default_account();
        let new_authorized_account = create_default_account();
        let transaction_accounts = vec![
            (vote_pubkey, vote_account),
            (clock_address, clock_account),
            (default_authorized_pubkey, authorized_account),
            (new_authorized_pubkey, new_authorized_account),
        ];
        let instruction_accounts = vec![
            AccountMeta {
                pubkey: vote_pubkey,
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: clock_address,
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: default_authorized_pubkey,
                is_signer: true,
                is_writable: false,
            },
            AccountMeta {
                pubkey: new_authorized_pubkey,
                is_signer: true,
                is_writable: false,
            },
        ];
        process_instruction(
            &serialize(&VoteInstruction::AuthorizeChecked(VoteAuthorize::Voter)).unwrap(),
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Ok(()),
        );
        process_instruction(
            &serialize(&VoteInstruction::AuthorizeChecked(
                VoteAuthorize::Withdrawer,
            ))
            .unwrap(),
            transaction_accounts,
            instruction_accounts,
            Ok(()),
        );
    }
}
