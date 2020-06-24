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
pub fn assign(assigned_account: &Pubkey, owner: &Pubkey) -> Instruction {
    let account_metas = vec![AccountMeta::new(*assigned_account, true)];
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

#[instructions(system_program::id())]
pub enum SystemInstruction {
    /// Assign account to a program
    #[accounts([
        (assigned_account, [WRITE, SIGNER], "Assigned account public key"),
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
/// * `assigned_account` - [WRITE, SIGNER] Assigned account public key
/// * `owner` - Owner program account
pub fn assign(assigned_account: Pubkey, owner: Pubkey) -> Instruction {
    let account_metas = vec![AccountMeta::new(assigned_account, true)];
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

## Next steps

A mod attribute:

```rust,ignore
#[program]
mod system {
    /// Assign account to a program
    #[accounts([
        (assigned_account, [WRITE, SIGNER], "Assigned account public key"),
    ])]
    fn assign(
        assigned_account: &KeyedAccount,
        owner: &Pubkey,
        signers: &HashSet<Pubkey>,
    ) -> Result<(), InstructionError> {
        // no work to do, just return
        if assigned_account.account.owner == *owner {
            return Ok(());
        }

        if !address.is_signer(&signers) {
            debug!("Assign: account must sign");
            return Err(InstructionError::MissingRequiredSignature);
        }

        // guard against sysvars being made
        if sysvar::check_id(&owner) {
            debug!("Assign: program id {} invalid", owner);
            return Err(SystemError::InvalidProgramId.into());
        }

        assigned_account.account.owner = *owner;
        Ok(())
    }
}
```

To generate the `process_instruction` implementation and the enum,
complete with `accounts` attribute:

```rust,ignore
#[instructions(system_program::id())]
pub enum SystemInstruction {
    /// Assign account to a program
    #[accounts([
        (assigned_account, [WRITE, SIGNER], "Assigned account public key"),
    ])]
    Assign {
        /// Owner program account
        owner: Pubkey,
    },
}

mod system {
    /// Assign account to a program
    fn assign(
        assigned_account: &KeyedAccount,
        owner: &Pubkey,
        signers: &HashSet<Pubkey>,
    ) -> Result<(), InstructionError> {
        // no work to do, just return
        if assigned_account.account.owner == *owner {
            return Ok(());
        }

        if !address.is_signer(&signers) {
            debug!("Assign: account must sign");
            return Err(InstructionError::MissingRequiredSignature);
        }

        // guard against sysvars being made
        if sysvar::check_id(&owner) {
            debug!("Assign: program id {} invalid", owner);
            return Err(SystemError::InvalidProgramId.into());
        }

        assigned_account.account.owner = *owner;
        Ok(())
    }
}

pub fn process_instruction(
    _owner: &Pubkey,
    keyed_accounts: &[KeyedAccount],
    instruction_data: &[u8],
) -> Result<(), InstructionError> {
    let instruction = limited_deserialize(instruction_data)?;
    let signers = get_signers(keyed_accounts);
    let keyed_accounts_iter = &mut keyed_accounts.iter();

    match instruction {
       SystemInstruction::Assign { owner } => {
            let keyed_account = next_keyed_account(keyed_accounts_iter)?;
            system::assign(&keyed_account, &owner, &signers)
       }
    }
}
```
