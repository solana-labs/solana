use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    program_error::ProgramError,
    pubkey::Pubkey,
};
use solana_sdk_program_macros::instructions;

#[instructions(test_program::id())]
#[derive(Clone, Debug, PartialEq)]
pub enum CustomSerdeInstruction {
    /// Transfer lamports
    #[accounts(
        from_account(SIGNER, WRITABLE, desc = "Funding account"),
        to_account(WRITABLE, desc = "Recipient account")
    )]
    Variant,

    /// Provide one required signature and a variable list of other signatures
    #[accounts(
        required_account(WRITABLE, desc = "Required account"),
        signers(SIGNER, multiple, desc = "Signer")
    )]
    MultipleAccounts,

    /// Consumes a stored nonce, replacing it with a successor
    #[accounts(
        required_account(SIGNER, WRITABLE, desc = "Required account"),
        sysvar(desc = "Sysvar"),
        authority(SIGNER, optional, desc = "Authority")
    )]
    OptionalAccount,

    /// Skip this variant in helper-function and verbose-enum expansion
    #[accounts(required_account(SIGNER, WRITABLE, desc = "Required account"))]
    #[skip]
    SkipVariant,
}

impl CustomSerdeInstruction {
    pub fn serialize(self: &Self) -> Result<Vec<u8>, ProgramError> {
        let mut output = vec![0u8; 1];
        match self {
            Self::Variant => output[0] = 0,
            Self::MultipleAccounts => output[0] = 1,
            Self::OptionalAccount => output[0] = 2,
            Self::SkipVariant => output[0] = 3,
        }
        Ok(output)
    }
}

mod test_program {
    solana_sdk::declare_id!("8dGutFWpfHymgGDV6is389USqGRqSfpGZyhBrF1VPWDg");
}

#[test]
fn test_helper_fns_custom_serde() {
    let pubkey0 = Pubkey::new_rand();
    let pubkey1 = Pubkey::new_rand();
    let pubkey2 = Pubkey::new_rand();

    assert_eq!(
        variant(pubkey0, pubkey1),
        Instruction {
            program_id: test_program::id(),
            accounts: vec![
                AccountMeta {
                    pubkey: pubkey0,
                    is_signer: true,
                    is_writable: true,
                },
                AccountMeta {
                    pubkey: pubkey1,
                    is_signer: false,
                    is_writable: true,
                }
            ],
            data: CustomSerdeInstruction::Variant.serialize().unwrap(),
        }
    );

    assert_eq!(
        multiple_accounts(pubkey0, vec![pubkey1, pubkey2]),
        Instruction {
            program_id: test_program::id(),
            accounts: vec![
                AccountMeta {
                    pubkey: pubkey0,
                    is_signer: false,
                    is_writable: true,
                },
                AccountMeta {
                    pubkey: pubkey1,
                    is_signer: true,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: pubkey2,
                    is_signer: true,
                    is_writable: false,
                }
            ],
            data: CustomSerdeInstruction::MultipleAccounts
                .serialize()
                .unwrap(),
        }
    );

    assert_eq!(
        multiple_accounts(pubkey0, vec![]),
        Instruction {
            program_id: test_program::id(),
            accounts: vec![AccountMeta {
                pubkey: pubkey0,
                is_signer: false,
                is_writable: true,
            }],
            data: CustomSerdeInstruction::MultipleAccounts
                .serialize()
                .unwrap(),
        }
    );

    assert_eq!(
        optional_account(pubkey0, pubkey1, Some(pubkey2)),
        Instruction {
            program_id: test_program::id(),
            accounts: vec![
                AccountMeta {
                    pubkey: pubkey0,
                    is_signer: true,
                    is_writable: true,
                },
                AccountMeta {
                    pubkey: pubkey1,
                    is_signer: false,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: pubkey2,
                    is_signer: true,
                    is_writable: false,
                }
            ],
            data: CustomSerdeInstruction::OptionalAccount.serialize().unwrap(),
        }
    );

    assert_eq!(
        optional_account(pubkey0, pubkey1, None),
        Instruction {
            program_id: test_program::id(),
            accounts: vec![
                AccountMeta {
                    pubkey: pubkey0,
                    is_signer: true,
                    is_writable: true,
                },
                AccountMeta {
                    pubkey: pubkey1,
                    is_signer: false,
                    is_writable: false,
                }
            ],
            data: CustomSerdeInstruction::OptionalAccount.serialize().unwrap(),
        }
    );
}

#[test]
fn test_from_instruction_custom_serde() {
    let transfer = CustomSerdeInstruction::Variant;
    let verbose_transfer =
        CustomSerdeInstructionVerbose::from_instruction(transfer, vec![2, 3]).unwrap();
    assert_eq!(
        verbose_transfer,
        CustomSerdeInstructionVerbose::Variant {
            from_account: 2,
            to_account: 3,
        }
    );

    let multiple = CustomSerdeInstruction::MultipleAccounts;
    let verbose_multiple =
        CustomSerdeInstructionVerbose::from_instruction(multiple, vec![2, 3, 4]).unwrap();
    assert_eq!(
        verbose_multiple,
        CustomSerdeInstructionVerbose::MultipleAccounts {
            required_account: 2,
            signers: vec![3, 4],
        }
    );

    let optional = CustomSerdeInstruction::OptionalAccount;
    let verbose_optional =
        CustomSerdeInstructionVerbose::from_instruction(optional, vec![2, 3, 4]).unwrap();
    assert_eq!(
        verbose_optional,
        CustomSerdeInstructionVerbose::OptionalAccount {
            required_account: 2,
            sysvar: 3,
            authority: Some(4),
        }
    );

    let skip = CustomSerdeInstruction::SkipVariant;
    assert!(CustomSerdeInstructionVerbose::from_instruction(skip, vec![0]).is_err());
}
