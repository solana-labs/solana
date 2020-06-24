# Program Attributes

## Problem

Instructions are currently defined with an `enum`:

```rust,ignore
pub enum SystemInstruction {
    /// Assign account to a program
    ///
    /// # Account references
    ///   0. [WRITE, SIGNER] Assigned account public key
    Assign {
        /// Owner program account
        owner: Pubkey,
    },
}
```

and a constructor:

```rust,ignore
pub fn assign(pubkey: &Pubkey, owner: &Pubkey) -> Instruction {
    let account_metas = vec![AccountMeta::new(*pubkey, true)];
    Instruction::new(
        system_program::id(),
        &SystemInstruction::Assign { owner: *owner },
        account_metas,
    )
}
```

Nearly everything in that constructor is duplicate information, but we
currently can't generate that constructor from the enum definition because
the list of account references is in code comments.

## Proposed Solution

Move the data from code comments to attributes, such that the constructors
can be generated, and include all the documentation from the enum definition.


```rust,ignore
use solana_sdk::{
    program_macros::{program, accounts, WRITE, SIGNER}
    pubkey::Pubkey;
};

#[program(system_program::id())]
pub enum SystemInstruction {
    /// Assign account to a program
    #[accounts([
        (pubkey, [WRITE, SIGNER], "Assigned account public key"),
    ])]
    Assign {
        /// Owner program account
        owner: Pubkey,
    },
}
```

This should generate the following code:

```rust,ignore
pub enum SystemInstruction {
    /// Assign account to a program
    ///
    /// # Account references
    ///   0. [WRITE, SIGNER] Assigned account public key
    Assign {
        /// Owner program account
        owner: Pubkey,
    },
}

/// Assign account to a program
///
/// * `pubkey` - [WRITE, SIGNER] Assigned account public key
/// * `owner` - Owner program account
pub fn assign(pubkey: Pubkey, owner: Pubkey) -> Instruction {
    let account_metas = vec![AccountMeta::new(pubkey, true)];
    Instruction::new(
        system_program::id(),
        &SystemInstruction::Assign { owner },
        account_metas,
    )
}
```

Algorithm:

1. `program` macro requires the program ID to pass to `Instruction::new`
2. A separate instruction constructor is created for each enum value
3. The constructor name is the `snake_case` version of the instruction name
4. `accounts` macro allows for the generation of `AccountMeta`.

Considerations:

* The pain that will come from generating `Pubkey` instead of `&Pubkey`
* How to denote optional accounts?
* How to denote "varargs", all remaining accounts, and which should be signed
  or writable?
