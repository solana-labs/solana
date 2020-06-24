# Program Instruction Macro

## Problem

Currently, inspecting an on-chain transaction requires depending on a
client-side, language-specific decoding library to parse the instruction.  If
rpc methods could return decoded instruction details, these custom solutions
would be unnecessary.

We can deserialize instruction data using a program's Instruction enum, but
decoding the account-key list into human-readable identifiers requires manual
parsing. Our current Instruction enums have that account information, but only
in variant docs.

Also, Instruction docs can vary between implementations, as there is no
mechanism to ensure consistency.

## Proposed Solution

Implement a procedural macro that parses an Instruction enum and generates a
second, verbose enum that contains account details and implements a conversion
method between the two enums.

Parsing the account documentation as it is would be brittle; a better method
would be to store account information in an  attribute for each Instruction
variant, which could be checked for correctness on compile. The macro would
parse this `accounts` attribute to generate the account fields on the new enum,
as well as generate pretty, consistent documentation for the Instruction enum
itself.

The macro could also support a `verbose_derive` item-level attribute in order to
enable custom configuration of derived traits for the verbose enum.

Here is an example of an Instruction enum using the new accounts format:

```text
#[repr(C)]
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, ProgramInstruction)]
#[verbose_derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum TestInstruction {
    /// Consumes a stored nonce, replacing it with a successor
    #[accounts(
        nonce_account(is_signer = true, is_writable = true, desc = "Nonce account"),
        recent_blockhashes_sysvar(desc = "RecentBlockhashes sysvar"),
        nonce_authority(is_signer = true, desc = "Nonce authority"),
    )]
    AdvanceNonceAccount,

    /// Transfer lamports
    #[accounts(
        funding_account(is_signer = true, is_writable = true, desc = "Funding account"),
        recipient_account(is_writable = true, desc = "Recipient account"),
    )]
    Transfer {
        lamports: u64,
    },

    /// Drive state of Uninitalized nonce account to Initialized, setting the nonce value
    ///
    /// No signatures are required to execute this instruction, enabling derived
    /// nonce account addresses
    #[accounts(
        nonce_account(is_signer = true,is_writable = true, desc = "Nonce account"),
        recent_blockhashes_sysvar(desc = "RecentBlockhashes sysvar"),
        rent_sysvar(desc = "Rent sysvar"),
    )]
    InitializeNonceAccount {
        /// Specifies the entity authorized to execute nonce instruction on the account
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
        let verbose_initialize  =
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
```

An example of the generated TestInstruction docs for AdvanceNonceAccount:
```text
    /// Consumes a stored nonce, replacing it with a successor
    ///
    /// * Accounts expected by this instruction:
    ///   0. `[writable, signer]` Nonce account
    ///   1. `[]` RecentBlockhashes sysvar
    ///   2. `[signer]` Nonce authority
    AdvanceNonceAccount,
```

Generated TestInstructionVerbose enum:

```text
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum TestInstruction {
    /// Consumes a stored nonce, replacing it with a successor
    AdvanceNonceAccount {
        /// Nonce account
        nonce_account: u8

        /// RecentBlockhashes sysvar
        recent_blockhashes_sysvar: u8

        /// Nonce authority
        nonce_authority: u8
    },

    /// Transfer lamports
    Transfer {
        /// Funding account
        funding_account: u8

        /// Recipient account
        recipient_account: u8

        lamports: u64,
    },

    /// Drive state of Uninitialized nonce account to Initialized, setting the nonce value
    ///
    /// No signatures are required to execute this instruction, enabling derived
    /// nonce account addresses
    #[accounts(
        nonce_account(is_signer = true,is_writable = true, desc = "Nonce account"),
        recent_blockhashes_sysvar(desc = "RecentBlockhashes sysvar"),
        rent_sysvar(desc = "Rent sysvar"),
    )]
    InitializeNonceAccount {
        /// Nonce account
        nonce_account: u8

        /// RecentBlockhashes sysvar
        recent_blockhashes_sysvar: u8

        /// Rent sysvar
        rent_sysvar: u8

        /// Specifies the entity authorized to execute nonce instruction on the account
        pubkey: Pubkey,
    },
}

impl TestInstructionVerbose {
    pub fn from_instruction(instruction: TestInstruction, account_keys: Vec<u8>) -> Self {
        match instruction {
            TestInstruction::AdvanceNonceAccount => TestInstructionVerbose::AdvanceNonceAccount {
                nonce_account: account_keys[0],
                recent_blockhashes_sysvar: account_keys[1],
                nonce_authority: account_keys[2],
            }
            TestInstruction::Transfer { lamports } => TestInstructionVerbose::Transfer {
                funding_account: account_keys[0],
                recipient_account: account_keys[1],
                lamports,
            }
            TestInstruction::InitializeNonceAccount => TestInstructionVerbose::InitializeNonceAccount {
                nonce_account: account_keys[0],
                recent_blockhashes_sysvar: account_keys[1],
                rent_sysvar: account_keys[2],
                pubkey,
            }
        }
    }
}

```

## Considerations

1. **Named fields** - Since the resulting Verbose enum constructs variants with
named fields, any unnamed fields in the original Instruction variant will need
to have names generated. As such, it would be considerably more straightforward
if all Instruction enum fields are converted to named types, instead of unnamed
tuples. This seems worth doing anyway, adding more precision to the variants and
enabling real documentation (so developers don't have to do
[this](https://github.com/solana-labs/solana/blob/3aab13a1679ba2b7846d9ba39b04a52f2017d3e0/sdk/src/system_instruction.rs#L140)
This will cause a little churn in our current code base, but not a lot.
2. **Variable account lists** - This approach only handles account lists of known
length and composition; it doesn't offer a solution for variable account lists,
like
[spl-token](https://github.com/solana-labs/solana-program-library/blob/master/token/src/instruction.rs#L30).
With the exception of that one TokenInstruction variant, all the other existing
native and bpf Instruction variants support static accounts lists. So one
possible solution would be to require all Instruction variants to do so, forcing
TokenInstruction to implement a new variant, like `NewTokenWithMint`.
