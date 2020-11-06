use crate::{config, legacy_stake_state::StakeAccount, stake_instruction::StakeInstruction};
use log::*;
use solana_sdk::{
    account::{get_signers, next_keyed_account, KeyedAccount},
    instruction::InstructionError,
    program_utils::limited_deserialize,
    pubkey::Pubkey,
    sysvar::{clock::Clock, rent::Rent, stake_history::StakeHistory, Sysvar},
};

pub fn process_instruction(
    _program_id: &Pubkey,
    keyed_accounts: &[KeyedAccount],
    data: &[u8],
) -> Result<(), InstructionError> {
    trace!("process_instruction: {:?}", data);
    trace!("keyed_accounts: {:?}", keyed_accounts);

    let signers = get_signers(keyed_accounts);

    let keyed_accounts = &mut keyed_accounts.iter();
    let me = &next_keyed_account(keyed_accounts)?;

    match limited_deserialize(data)? {
        StakeInstruction::Initialize(authorized, lockup) => me.initialize(
            &authorized,
            &lockup,
            &Rent::from_keyed_account(next_keyed_account(keyed_accounts)?)?,
        ),
        StakeInstruction::Authorize(authorized_pubkey, stake_authorize) => {
            me.authorize(&signers, &authorized_pubkey, stake_authorize)
        }
        StakeInstruction::AuthorizeWithSeed(args) => {
            let authority_base = next_keyed_account(keyed_accounts)?;
            me.authorize_with_seed(
                &authority_base,
                &args.authority_seed,
                &args.authority_owner,
                &args.new_authorized_pubkey,
                args.stake_authorize,
            )
        }
        StakeInstruction::DelegateStake => {
            let vote = next_keyed_account(keyed_accounts)?;

            me.delegate(
                &vote,
                &Clock::from_keyed_account(next_keyed_account(keyed_accounts)?)?,
                &StakeHistory::from_keyed_account(next_keyed_account(keyed_accounts)?)?,
                &config::from_keyed_account(next_keyed_account(keyed_accounts)?)?,
                &signers,
            )
        }
        StakeInstruction::Split(lamports) => {
            let split_stake = &next_keyed_account(keyed_accounts)?;
            me.split(lamports, split_stake, &signers)
        }
        StakeInstruction::Merge => {
            let source_stake = &next_keyed_account(keyed_accounts)?;
            me.merge(
                source_stake,
                &Clock::from_keyed_account(next_keyed_account(keyed_accounts)?)?,
                &StakeHistory::from_keyed_account(next_keyed_account(keyed_accounts)?)?,
                &signers,
            )
        }

        StakeInstruction::Withdraw(lamports) => {
            let to = &next_keyed_account(keyed_accounts)?;
            me.withdraw(
                lamports,
                to,
                &Clock::from_keyed_account(next_keyed_account(keyed_accounts)?)?,
                &StakeHistory::from_keyed_account(next_keyed_account(keyed_accounts)?)?,
                next_keyed_account(keyed_accounts)?,
                keyed_accounts.next(),
            )
        }
        StakeInstruction::Deactivate => me.deactivate(
            &Clock::from_keyed_account(next_keyed_account(keyed_accounts)?)?,
            &signers,
        ),

        StakeInstruction::SetLockup(lockup) => me.set_lockup(&lockup, &signers),
    }
}
