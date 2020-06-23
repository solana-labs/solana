use solana_sdk::pubkey::Pubkey;
use solana_sdk_macro::ProgramInstruction;

#[repr(C)]
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, ProgramInstruction)]
#[instruction_derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum TestInstructionVerbose {
    #[doc = "Consumes a stored nonce, replacing it with a successor"]
    AdvanceNonceAccount {
        #[account(signer, writable)]
        #[desc = "Nonce account"]
        #[index = 0]
        nonce_account: u8,

        #[account]
        #[desc = "RecentBlockhashes sysvar"]
        #[index = 1]
        recent_blockhashes_sysvar: u8,

        #[account(signer)]
        #[desc = "Nonce authority"]
        #[index = 2]
        nonce_authority: u8,
    },
    #[doc = "Transfer lamports"]
    Transfer {
        #[account(signer, writable)]
        #[desc = "Funding account"]
        #[index = 0]
        funding_account: u8,

        #[account(writable)]
        #[desc = "Recipient account"]
        #[index = 1]
        recipient_account: u8,

        lamports: u64,
    },
    #[doc = "Drive state of Uninitalized nonce account to Initialized, setting the nonce value"]
    #[doc = "No signatures are required to execute this instruction, enabling derived nonce account addresses"]
    InitializeNonceAccount {
        #[account(signer, writable)]
        #[desc = "Nonce account"]
        #[index = 0]
        nonce_account: u8,

        #[account]
        #[desc = "RecentBlockhashes sysvar"]
        #[index = 1]
        recent_blockhashes_sysvar: u8,

        #[account]
        #[desc = "Rent sysvar"]
        #[index = 2]
        rent_sysvar: u8,

        #[doc = "The `Pubkey` parameter specifies the entity authorized to execute nonce instruction on the account"]
        pubkey: Pubkey,
    },
}

mod tests {

    #[test]
    fn test_from() {
        use super::*;
        let transfer = TestInstruction::Transfer { lamports: 42 };
        let verbose_transfer = TestInstructionVerbose::from_instruction(transfer, vec![2, 3]);
        assert_eq!(
            verbose_transfer,
            TestInstructionVerbose::Transfer {
                funding_account: 2,
                recipient_account: 3,
                lamports: 42
            }
        );

        let advance = TestInstruction::AdvanceNonceAccount;
        let verbose_advance = TestInstructionVerbose::from_instruction(advance, vec![2, 3, 4]);
        assert_eq!(
            verbose_advance,
            TestInstructionVerbose::AdvanceNonceAccount {
                nonce_account: 2,
                recent_blockhashes_sysvar: 3,
                nonce_authority: 4,
            }
        );

        let nonce_address = Pubkey::new_rand();
        let initialize = TestInstruction::InitializeNonceAccount {
            pubkey: nonce_address,
        };
        let verbose_initialize =
            TestInstructionVerbose::from_instruction(initialize, vec![2, 3, 4]);
        assert_eq!(
            verbose_initialize,
            TestInstructionVerbose::InitializeNonceAccount {
                nonce_account: 2,
                recent_blockhashes_sysvar: 3,
                rent_sysvar: 4,
                pubkey: nonce_address,
            }
        );
    }
}
