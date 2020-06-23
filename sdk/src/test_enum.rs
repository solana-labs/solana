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
