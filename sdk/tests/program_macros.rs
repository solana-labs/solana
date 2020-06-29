use bincode::serialize;
use serde_derive::{Deserialize, Serialize};
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
};
use solana_sdk_program_macros::instructions;

#[instructions(test_program::id())]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum TestInstruction {
    /// Transfer lamports
    #[accounts(
        from_account(SIGNER, WRITABLE, desc = "Funding account"),
        to_account(WRITABLE, desc = "Recipient account")
    )]
    Transfer {
        /// Test a field comment
        lamports: u64,
    },

    /// Provide one required signature and a variable list of other signatures
    #[accounts(
        required_account(WRITABLE, desc = "Required account"),
        signers(SIGNER, multiple, desc = "Signer")
    )]
    MultipleAccounts,

    /// Do some action with 2 required accounts and one optional account
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

mod test_program {
    solana_sdk::declare_id!("8dGutFWpfHymgGDV6is389USqGRqSfpGZyhBrF1VPWDg");
}

#[test]
fn test_helper_fns() {
    let pubkey0 = Pubkey::new_rand();
    let pubkey1 = Pubkey::new_rand();
    let pubkey2 = Pubkey::new_rand();

    assert_eq!(
        transfer(pubkey0, pubkey1, 42),
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
            data: serialize(&TestInstruction::Transfer { lamports: 42 }).unwrap(),
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
            data: serialize(&TestInstruction::MultipleAccounts).unwrap(),
        }
    );

    assert_eq!(
        multiple_accounts(pubkey0, vec![]),
        Instruction {
            program_id: test_program::id(),
            accounts: vec![
                AccountMeta {
                    pubkey: pubkey0,
                    is_signer: false,
                    is_writable: true,
                }
            ],
            data: serialize(&TestInstruction::MultipleAccounts).unwrap(),
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
            data: serialize(&TestInstruction::OptionalAccount).unwrap(),
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
            data: serialize(&TestInstruction::OptionalAccount).unwrap(),
        }
    );
}

#[test]
fn test_from_instruction() {
    let transfer = TestInstruction::Transfer { lamports: 42 };
    let verbose_transfer = TestInstructionVerbose::from_instruction(transfer, vec![2, 3]).unwrap();
    assert_eq!(
        verbose_transfer,
        TestInstructionVerbose::Transfer {
            from_account: 2,
            to_account: 3,
            lamports: 42,
        }
    );

    let multiple = TestInstruction::MultipleAccounts;
    let verbose_multiple =
        TestInstructionVerbose::from_instruction(multiple, vec![2, 3, 4]).unwrap();
    assert_eq!(
        verbose_multiple,
        TestInstructionVerbose::MultipleAccounts {
            required_account: 2,
            signers: vec![3, 4],
        }
    );

    let optional = TestInstruction::OptionalAccount;
    let verbose_optional =
        TestInstructionVerbose::from_instruction(optional, vec![2, 3, 4]).unwrap();
    assert_eq!(
        verbose_optional,
        TestInstructionVerbose::OptionalAccount {
            required_account: 2,
            sysvar: 3,
            authority: Some(4),
        }
    );

    let skip = TestInstruction::SkipVariant;
    assert!(TestInstructionVerbose::from_instruction(skip, vec![0]).is_err());
}
