use crate::parse_instruction::{
    check_num_accounts, ParsableProgram, ParseInstructionError, ParsedInstructionEnum,
};
use bincode::deserialize;
use serde_json::{json, Map};
use solana_sdk::{instruction::CompiledInstruction, pubkey::Pubkey};
use solana_stake_program::stake_instruction::StakeInstruction;

pub fn parse_stake(
    instruction: &CompiledInstruction,
    account_keys: &[Pubkey],
) -> Result<ParsedInstructionEnum, ParseInstructionError> {
    let stake_instruction: StakeInstruction = deserialize(&instruction.data)
        .map_err(|_| ParseInstructionError::InstructionNotParsable(ParsableProgram::Stake))?;
    match instruction.accounts.iter().max() {
        Some(index) if (*index as usize) < account_keys.len() => {}
        _ => {
            // Runtime should prevent this from ever happening
            return Err(ParseInstructionError::InstructionKeyMismatch(
                ParsableProgram::Stake,
            ));
        }
    }
    match stake_instruction {
        StakeInstruction::Initialize(authorized, lockup) => {
            check_num_stake_accounts(&instruction.accounts, 2)?;
            let authorized = json!({
                "staker": authorized.staker.to_string(),
                "withdrawer": authorized.withdrawer.to_string(),
            });
            let lockup = json!({
                "unixTimestamp": lockup.unix_timestamp,
                "epoch": lockup.epoch,
                "custodian": lockup.custodian.to_string(),
            });
            Ok(ParsedInstructionEnum {
                instruction_type: "initialize".to_string(),
                info: json!({
                    "stakeAccount": account_keys[instruction.accounts[0] as usize].to_string(),
                    "rentSysvar": account_keys[instruction.accounts[1] as usize].to_string(),
                    "authorized": authorized,
                    "lockup": lockup,
                }),
            })
        }
        StakeInstruction::Authorize(new_authorized, authority_type) => {
            check_num_stake_accounts(&instruction.accounts, 3)?;
            let mut value = json!({
                "stakeAccount": account_keys[instruction.accounts[0] as usize].to_string(),
                "clockSysvar": account_keys[instruction.accounts[1] as usize].to_string(),
                "authority": account_keys[instruction.accounts[2] as usize].to_string(),
                "newAuthority": new_authorized.to_string(),
                "authorityType": authority_type,
            });
            let map = value.as_object_mut().unwrap();
            if instruction.accounts.len() >= 4 {
                map.insert(
                    "custodian".to_string(),
                    json!(account_keys[instruction.accounts[3] as usize].to_string()),
                );
            }
            Ok(ParsedInstructionEnum {
                instruction_type: "authorize".to_string(),
                info: value,
            })
        }
        StakeInstruction::DelegateStake => {
            check_num_stake_accounts(&instruction.accounts, 6)?;
            Ok(ParsedInstructionEnum {
                instruction_type: "delegate".to_string(),
                info: json!({
                    "stakeAccount": account_keys[instruction.accounts[0] as usize].to_string(),
                    "voteAccount": account_keys[instruction.accounts[1] as usize].to_string(),
                    "clockSysvar": account_keys[instruction.accounts[2] as usize].to_string(),
                    "stakeHistorySysvar": account_keys[instruction.accounts[3] as usize].to_string(),
                    "stakeConfigAccount": account_keys[instruction.accounts[4] as usize].to_string(),
                    "stakeAuthority": account_keys[instruction.accounts[5] as usize].to_string(),
                }),
            })
        }
        StakeInstruction::Split(lamports) => {
            check_num_stake_accounts(&instruction.accounts, 3)?;
            Ok(ParsedInstructionEnum {
                instruction_type: "split".to_string(),
                info: json!({
                    "stakeAccount": account_keys[instruction.accounts[0] as usize].to_string(),
                    "newSplitAccount": account_keys[instruction.accounts[1] as usize].to_string(),
                    "stakeAuthority": account_keys[instruction.accounts[2] as usize].to_string(),
                    "lamports": lamports,
                }),
            })
        }
        StakeInstruction::Withdraw(lamports) => {
            check_num_stake_accounts(&instruction.accounts, 5)?;
            let mut value = json!({
                "stakeAccount": account_keys[instruction.accounts[0] as usize].to_string(),
                "destination": account_keys[instruction.accounts[1] as usize].to_string(),
                "clockSysvar": account_keys[instruction.accounts[2] as usize].to_string(),
                "stakeHistorySysvar": account_keys[instruction.accounts[3] as usize].to_string(),
                "withdrawAuthority": account_keys[instruction.accounts[4] as usize].to_string(),
                "lamports": lamports,
            });
            let map = value.as_object_mut().unwrap();
            if instruction.accounts.len() >= 6 {
                map.insert(
                    "custodian".to_string(),
                    json!(account_keys[instruction.accounts[5] as usize].to_string()),
                );
            }
            Ok(ParsedInstructionEnum {
                instruction_type: "withdraw".to_string(),
                info: value,
            })
        }
        StakeInstruction::Deactivate => {
            check_num_stake_accounts(&instruction.accounts, 3)?;
            Ok(ParsedInstructionEnum {
                instruction_type: "deactivate".to_string(),
                info: json!({
                    "stakeAccount": account_keys[instruction.accounts[0] as usize].to_string(),
                    "clockSysvar": account_keys[instruction.accounts[1] as usize].to_string(),
                    "stakeAuthority": account_keys[instruction.accounts[2] as usize].to_string(),
                }),
            })
        }
        StakeInstruction::SetLockup(lockup_args) => {
            check_num_stake_accounts(&instruction.accounts, 2)?;
            let mut lockup_map = Map::new();
            if let Some(timestamp) = lockup_args.unix_timestamp {
                lockup_map.insert("unixTimestamp".to_string(), json!(timestamp));
            }
            if let Some(epoch) = lockup_args.epoch {
                lockup_map.insert("epoch".to_string(), json!(epoch));
            }
            if let Some(custodian) = lockup_args.custodian {
                lockup_map.insert("custodian".to_string(), json!(custodian.to_string()));
            }
            Ok(ParsedInstructionEnum {
                instruction_type: "setLockup".to_string(),
                info: json!({
                    "stakeAccount": account_keys[instruction.accounts[0] as usize].to_string(),
                    "custodian": account_keys[instruction.accounts[1] as usize].to_string(),
                    "lockup": lockup_map,
                }),
            })
        }
        StakeInstruction::Merge => {
            check_num_stake_accounts(&instruction.accounts, 5)?;
            Ok(ParsedInstructionEnum {
                instruction_type: "merge".to_string(),
                info: json!({
                    "destination": account_keys[instruction.accounts[0] as usize].to_string(),
                    "source": account_keys[instruction.accounts[1] as usize].to_string(),
                    "clockSysvar": account_keys[instruction.accounts[2] as usize].to_string(),
                    "stakeHistorySysvar": account_keys[instruction.accounts[3] as usize].to_string(),
                    "stakeAuthority": account_keys[instruction.accounts[4] as usize].to_string(),
                }),
            })
        }
        StakeInstruction::AuthorizeWithSeed(args) => {
            check_num_stake_accounts(&instruction.accounts, 2)?;
            let mut value = json!({
                    "stakeAccount": account_keys[instruction.accounts[0] as usize].to_string(),
                    "authorityBase": account_keys[instruction.accounts[1] as usize].to_string(),
                    "newAuthorized": args.new_authorized_pubkey.to_string(),
                    "authorityType": args.stake_authorize,
                    "authoritySeed": args.authority_seed,
                    "authorityOwner": args.authority_owner.to_string(),
            });
            let map = value.as_object_mut().unwrap();
            if instruction.accounts.len() >= 3 {
                map.insert(
                    "clockSysvar".to_string(),
                    json!(account_keys[instruction.accounts[2] as usize].to_string()),
                );
            }
            if instruction.accounts.len() >= 4 {
                map.insert(
                    "custodian".to_string(),
                    json!(account_keys[instruction.accounts[3] as usize].to_string()),
                );
            }
            Ok(ParsedInstructionEnum {
                instruction_type: "authorizeWithSeed".to_string(),
                info: value,
            })
        }
    }
}

fn check_num_stake_accounts(accounts: &[u8], num: usize) -> Result<(), ParseInstructionError> {
    check_num_accounts(accounts, num, ParsableProgram::Stake)
}

#[cfg(test)]
mod test {
    use super::*;
    use solana_sdk::{message::Message, pubkey::Pubkey};
    use solana_stake_program::{
        stake_instruction::{self, LockupArgs},
        stake_state::{Authorized, Lockup, StakeAuthorize},
    };

    #[test]
    #[allow(clippy::same_item_push)]
    fn test_parse_stake_instruction() {
        let mut keys: Vec<Pubkey> = vec![];
        for _ in 0..6 {
            keys.push(Pubkey::new_unique());
        }

        let authorized = Authorized {
            staker: Pubkey::new_unique(),
            withdrawer: Pubkey::new_unique(),
        };
        let lockup = Lockup {
            unix_timestamp: 1_234_567_890,
            epoch: 11,
            custodian: Pubkey::new_unique(),
        };
        let lamports = 55;

        let instructions =
            stake_instruction::create_account(&keys[0], &keys[1], &authorized, &lockup, lamports);
        let message = Message::new(&instructions, None);
        assert_eq!(
            parse_stake(&message.instructions[1], &keys[0..3]).unwrap(),
            ParsedInstructionEnum {
                instruction_type: "initialize".to_string(),
                info: json!({
                    "stakeAccount": keys[1].to_string(),
                    "rentSysvar": keys[2].to_string(),
                    "authorized": {
                        "staker": authorized.staker.to_string(),
                        "withdrawer": authorized.withdrawer.to_string(),
                    },
                    "lockup": {
                        "unixTimestamp": lockup.unix_timestamp,
                        "epoch": lockup.epoch,
                        "custodian": lockup.custodian.to_string(),
                    }
                }),
            }
        );
        assert!(parse_stake(&message.instructions[1], &keys[0..2]).is_err());

        let instruction = stake_instruction::authorize(
            &keys[1],
            &keys[0],
            &keys[3],
            StakeAuthorize::Staker,
            None,
        );
        let message = Message::new(&[instruction], None);
        assert_eq!(
            parse_stake(&message.instructions[0], &keys[0..3]).unwrap(),
            ParsedInstructionEnum {
                instruction_type: "authorize".to_string(),
                info: json!({
                    "stakeAccount": keys[1].to_string(),
                    "clockSysvar": keys[2].to_string(),
                    "authority": keys[0].to_string(),
                    "newAuthority": keys[3].to_string(),
                    "authorityType": StakeAuthorize::Staker,
                }),
            }
        );
        assert!(parse_stake(&message.instructions[0], &keys[0..2]).is_err());

        let instruction = stake_instruction::authorize(
            &keys[1],
            &keys[0],
            &keys[3],
            StakeAuthorize::Withdrawer,
            Some(&keys[1]),
        );
        let message = Message::new(&[instruction], None);
        assert_eq!(
            parse_stake(&message.instructions[0], &keys[0..3]).unwrap(),
            ParsedInstructionEnum {
                instruction_type: "authorize".to_string(),
                info: json!({
                    "stakeAccount": keys[1].to_string(),
                    "clockSysvar": keys[2].to_string(),
                    "authority": keys[0].to_string(),
                    "newAuthority": keys[3].to_string(),
                    "authorityType": StakeAuthorize::Withdrawer,
                    "custodian": keys[1].to_string(),
                }),
            }
        );
        assert!(parse_stake(&message.instructions[0], &keys[0..2]).is_err());

        let instruction = stake_instruction::delegate_stake(&keys[1], &keys[0], &keys[2]);
        let message = Message::new(&[instruction], None);
        assert_eq!(
            parse_stake(&message.instructions[0], &keys[0..6]).unwrap(),
            ParsedInstructionEnum {
                instruction_type: "delegate".to_string(),
                info: json!({
                    "stakeAccount": keys[1].to_string(),
                    "voteAccount": keys[2].to_string(),
                    "clockSysvar": keys[3].to_string(),
                    "stakeHistorySysvar": keys[4].to_string(),
                    "stakeConfigAccount": keys[5].to_string(),
                    "stakeAuthority": keys[0].to_string(),
                }),
            }
        );
        assert!(parse_stake(&message.instructions[0], &keys[0..5]).is_err());

        let instructions = stake_instruction::split(&keys[2], &keys[0], lamports, &keys[1]);
        let message = Message::new(&instructions, None);
        assert_eq!(
            parse_stake(&message.instructions[1], &keys[0..3]).unwrap(),
            ParsedInstructionEnum {
                instruction_type: "split".to_string(),
                info: json!({
                    "stakeAccount": keys[2].to_string(),
                    "newSplitAccount": keys[1].to_string(),
                    "stakeAuthority": keys[0].to_string(),
                    "lamports": lamports,
                }),
            }
        );
        assert!(parse_stake(&message.instructions[1], &keys[0..2]).is_err());

        let instruction = stake_instruction::withdraw(&keys[1], &keys[0], &keys[2], lamports, None);
        let message = Message::new(&[instruction], None);
        assert_eq!(
            parse_stake(&message.instructions[0], &keys[0..5]).unwrap(),
            ParsedInstructionEnum {
                instruction_type: "withdraw".to_string(),
                info: json!({
                    "stakeAccount": keys[1].to_string(),
                    "destination": keys[2].to_string(),
                    "clockSysvar": keys[3].to_string(),
                    "stakeHistorySysvar": keys[4].to_string(),
                    "withdrawAuthority": keys[0].to_string(),
                    "lamports": lamports,
                }),
            }
        );
        let instruction =
            stake_instruction::withdraw(&keys[2], &keys[0], &keys[3], lamports, Some(&keys[1]));
        let message = Message::new(&[instruction], None);
        assert_eq!(
            parse_stake(&message.instructions[0], &keys[0..6]).unwrap(),
            ParsedInstructionEnum {
                instruction_type: "withdraw".to_string(),
                info: json!({
                    "stakeAccount": keys[2].to_string(),
                    "destination": keys[3].to_string(),
                    "clockSysvar": keys[4].to_string(),
                    "stakeHistorySysvar": keys[5].to_string(),
                    "withdrawAuthority": keys[0].to_string(),
                    "custodian": keys[1].to_string(),
                    "lamports": lamports,
                }),
            }
        );
        assert!(parse_stake(&message.instructions[0], &keys[0..4]).is_err());

        let instruction = stake_instruction::deactivate_stake(&keys[1], &keys[0]);
        let message = Message::new(&[instruction], None);
        assert_eq!(
            parse_stake(&message.instructions[0], &keys[0..3]).unwrap(),
            ParsedInstructionEnum {
                instruction_type: "deactivate".to_string(),
                info: json!({
                    "stakeAccount": keys[1].to_string(),
                    "clockSysvar": keys[2].to_string(),
                    "stakeAuthority": keys[0].to_string(),
                }),
            }
        );
        assert!(parse_stake(&message.instructions[0], &keys[0..2]).is_err());

        let instructions = stake_instruction::merge(&keys[1], &keys[0], &keys[2]);
        let message = Message::new(&instructions, None);
        assert_eq!(
            parse_stake(&message.instructions[0], &keys[0..5]).unwrap(),
            ParsedInstructionEnum {
                instruction_type: "merge".to_string(),
                info: json!({
                    "destination": keys[1].to_string(),
                    "source": keys[2].to_string(),
                    "clockSysvar": keys[3].to_string(),
                    "stakeHistorySysvar": keys[4].to_string(),
                    "stakeAuthority": keys[0].to_string(),
                }),
            }
        );
        assert!(parse_stake(&message.instructions[0], &keys[0..4]).is_err());

        let seed = "test_seed";
        let instruction = stake_instruction::authorize_with_seed(
            &keys[1],
            &keys[0],
            seed.to_string(),
            &keys[2],
            &keys[3],
            StakeAuthorize::Staker,
            None,
        );
        let message = Message::new(&[instruction], None);
        assert_eq!(
            parse_stake(&message.instructions[0], &keys[0..3]).unwrap(),
            ParsedInstructionEnum {
                instruction_type: "authorizeWithSeed".to_string(),
                info: json!({
                    "stakeAccount": keys[1].to_string(),
                    "authorityOwner": keys[2].to_string(),
                    "newAuthorized": keys[3].to_string(),
                    "authorityBase": keys[0].to_string(),
                    "authoritySeed": seed,
                    "authorityType": StakeAuthorize::Staker,
                    "clockSysvar": keys[2].to_string(),
                }),
            }
        );
        assert!(parse_stake(&message.instructions[0], &keys[0..1]).is_err());

        let instruction = stake_instruction::authorize_with_seed(
            &keys[1],
            &keys[0],
            seed.to_string(),
            &keys[2],
            &keys[3],
            StakeAuthorize::Withdrawer,
            Some(&keys[4]),
        );
        let message = Message::new(&[instruction], None);
        assert_eq!(
            parse_stake(&message.instructions[0], &keys[0..5]).unwrap(),
            ParsedInstructionEnum {
                instruction_type: "authorizeWithSeed".to_string(),
                info: json!({
                    "stakeAccount": keys[2].to_string(),
                    "authorityOwner": keys[2].to_string(),
                    "newAuthorized": keys[3].to_string(),
                    "authorityBase": keys[0].to_string(),
                    "authoritySeed": seed,
                    "authorityType": StakeAuthorize::Withdrawer,
                    "clockSysvar": keys[3].to_string(),
                    "custodian": keys[1].to_string(),
                }),
            }
        );
        assert!(parse_stake(&message.instructions[0], &keys[0..1]).is_err());
    }

    #[test]
    #[allow(clippy::same_item_push)]
    fn test_parse_set_lockup() {
        let mut keys: Vec<Pubkey> = vec![];
        for _ in 0..2 {
            keys.push(Pubkey::new_unique());
        }
        let unix_timestamp = 1_234_567_890;
        let epoch = 11;
        let custodian = Pubkey::new_unique();

        let lockup = LockupArgs {
            unix_timestamp: Some(unix_timestamp),
            epoch: None,
            custodian: None,
        };
        let instruction = stake_instruction::set_lockup(&keys[1], &lockup, &keys[0]);
        let message = Message::new(&[instruction], None);
        assert_eq!(
            parse_stake(&message.instructions[0], &keys[0..2]).unwrap(),
            ParsedInstructionEnum {
                instruction_type: "setLockup".to_string(),
                info: json!({
                    "stakeAccount": keys[1].to_string(),
                    "custodian": keys[0].to_string(),
                    "lockup": {
                        "unixTimestamp": unix_timestamp
                    }
                }),
            }
        );

        let lockup = LockupArgs {
            unix_timestamp: Some(unix_timestamp),
            epoch: Some(epoch),
            custodian: None,
        };
        let instruction = stake_instruction::set_lockup(&keys[1], &lockup, &keys[0]);
        let message = Message::new(&[instruction], None);
        assert_eq!(
            parse_stake(&message.instructions[0], &keys[0..2]).unwrap(),
            ParsedInstructionEnum {
                instruction_type: "setLockup".to_string(),
                info: json!({
                    "stakeAccount": keys[1].to_string(),
                    "custodian": keys[0].to_string(),
                    "lockup": {
                        "unixTimestamp": unix_timestamp,
                        "epoch": epoch,
                    }
                }),
            }
        );

        let lockup = LockupArgs {
            unix_timestamp: Some(unix_timestamp),
            epoch: Some(epoch),
            custodian: Some(custodian),
        };
        let instruction = stake_instruction::set_lockup(&keys[1], &lockup, &keys[0]);
        let message = Message::new(&[instruction], None);
        assert_eq!(
            parse_stake(&message.instructions[0], &keys[0..2]).unwrap(),
            ParsedInstructionEnum {
                instruction_type: "setLockup".to_string(),
                info: json!({
                    "stakeAccount": keys[1].to_string(),
                    "custodian": keys[0].to_string(),
                    "lockup": {
                        "unixTimestamp": unix_timestamp,
                        "epoch": epoch,
                        "custodian": custodian.to_string(),
                    }
                }),
            }
        );

        assert!(parse_stake(&message.instructions[0], &keys[0..1]).is_err());
    }
}
