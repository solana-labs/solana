use crate::hash::hashv;
use crate::instruction::{AccountMeta, Instruction, WithSigner};
use crate::instruction_processor_utils::DecodeError;
use crate::nonce_state::NonceState;
use crate::pubkey::Pubkey;
use crate::system_program;
use crate::sysvar::{recent_blockhashes, rent};
use num_derive::{FromPrimitive, ToPrimitive};
use thiserror::Error;

#[derive(Serialize, Debug, Clone, PartialEq, FromPrimitive, ToPrimitive)]
pub enum SystemError {
    AccountAlreadyInUse,
    ResultWithNegativeLamports,
    InvalidProgramId,
    InvalidAccountDataLength,
    InvalidSeed,
    MaxSeedLengthExceeded,
    AddressWithSeedMismatch,
}

impl<T> DecodeError<T> for SystemError {
    fn type_of() -> &'static str {
        "SystemError"
    }
}

impl std::fmt::Display for SystemError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "error")
    }
}
impl std::error::Error for SystemError {}

#[derive(Error, Debug, Clone, PartialEq, FromPrimitive, ToPrimitive)]
pub enum NonceError {
    #[error("recent blockhash list is empty")]
    NoRecentBlockhashes,
    #[error("stored nonce is still in recent_blockhashes")]
    NotExpired,
    #[error("specified nonce does not match stored nonce")]
    UnexpectedValue,
    #[error("cannot handle request in current account state")]
    BadAccountState,
}

impl<E> DecodeError<E> for NonceError {
    fn type_of() -> &'static str {
        "NonceError"
    }
}

/// maximum length of derived address seed
pub const MAX_ADDRESS_SEED_LEN: usize = 32;

/// maximum permitted size of data: 10 MB
pub const MAX_PERMITTED_DATA_LENGTH: u64 = 10 * 1024 * 1024;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum SystemInstruction {
    /// Create a new account
    /// * Transaction::keys[0] - source
    /// * Transaction::keys[1] - new account key
    /// * lamports - number of lamports to transfer to the new account
    /// * space - number of bytes of memory to allocate
    /// * program_id - the program id of the new account
    CreateAccount {
        lamports: u64,
        space: u64,
        program_id: Pubkey,
    },
    /// Assign account to a program
    /// * Transaction::keys[0] - account to assign
    Assign { program_id: Pubkey },
    /// Transfer lamports
    /// * Transaction::keys[0] - source
    /// * Transaction::keys[1] - destination
    Transfer { lamports: u64 },
    /// Create a new account at an address derived from
    ///    a base pubkey and a seed
    /// * Transaction::keys[0] - source
    /// * Transaction::keys[1] - new account key
    /// * seed - string of ascii chars, no longer than MAX_ADDRESS_SEED_LEN
    /// * lamports - number of lamports to transfer to the new account
    /// * space - number of bytes of memory to allocate
    /// * program_id - the program id of the new account
    CreateAccountWithSeed {
        base: Pubkey,
        seed: String,
        lamports: u64,
        space: u64,
        program_id: Pubkey,
    },
    /// `AdvanceNonceAccount` consumes a stored nonce, replacing it with a successor
    ///
    /// Expects 2 Accounts:
    ///     0 - A NonceAccount
    ///     1 - RecentBlockhashes sysvar
    ///
    /// The current authority must sign a transaction executing this instrucion
    AdvanceNonceAccount,
    /// `WithdrawNonceAccount` transfers funds out of the nonce account
    ///
    /// Expects 4 Accounts:
    ///     0 - A NonceAccount
    ///     1 - A system account to which the lamports will be transferred
    ///     2 - RecentBlockhashes sysvar
    ///     3 - Rent sysvar
    ///
    /// The `u64` parameter is the lamports to withdraw, which must leave the
    /// account balance above the rent exempt reserve or at zero.
    ///
    /// The current authority must sign a transaction executing this instruction
    WithdrawNonceAccount(u64),
    /// `InitializeNonceAccount` drives state of Uninitalized NonceAccount to Initialized,
    /// setting the nonce value.
    ///
    /// Expects 3 Accounts:
    ///     0 - A NonceAccount in the Uninitialized state
    ///     1 - RecentBlockHashes sysvar
    ///     2 - Rent sysvar
    ///
    /// The `Pubkey` parameter specifies the entity authorized to execute nonce
    /// instruction on the account
    ///
    /// No signatures are required to execute this instruction, enabling derived
    /// nonce account addresses
    InitializeNonceAccount(Pubkey),
    /// `AuthorizeNonceAccount` changes the entity authorized to execute nonce instructions
    /// on the account
    ///
    /// Expects 1 Account:
    ///     0 - A NonceAccount
    ///
    /// The `Pubkey` parameter identifies the entity to authorize
    ///
    /// The current authority must sign a transaction executing this instruction
    AuthorizeNonceAccount(Pubkey),
    /// Allocate space in a (possibly new) account without funding
    /// * Transaction::keys[0] - new account key
    /// * space - number of bytes of memory to allocate
    Allocate { space: u64 },
    /// Allocate space for and assign an account at an address
    ///    derived from a base pubkey and a seed
    /// * Transaction::keys[0] - new account key
    /// * seed - string of ascii chars, no longer than MAX_ADDRESS_SEED_LEN
    /// * space - number of bytes of memory to allocate
    /// * program_id - the program id of the new account
    AllocateWithSeed {
        base: Pubkey,
        seed: String,
        space: u64,
        program_id: Pubkey,
    },
    /// Assign account to a program based on a seed
    /// * Transaction::keys[0] - account to assign
    /// * seed - string of ascii chars, no longer than MAX_ADDRESS_SEED_LEN
    /// * program_id - the program id of the new account
    AssignWithSeed {
        base: Pubkey,
        seed: String,
        program_id: Pubkey,
    },
}

pub fn create_account(
    from_pubkey: &Pubkey,
    to_pubkey: &Pubkey,
    lamports: u64,
    space: u64,
    program_id: &Pubkey,
) -> Instruction {
    let account_metas = vec![
        AccountMeta::new(*from_pubkey, true),
        AccountMeta::new(*to_pubkey, true),
    ];
    Instruction::new(
        system_program::id(),
        &SystemInstruction::CreateAccount {
            lamports,
            space,
            program_id: *program_id,
        },
        account_metas,
    )
}

// we accept `to` as a parameter so that callers do their own error handling when
//   calling create_address_with_seed()
pub fn create_account_with_seed(
    from_pubkey: &Pubkey,
    to_pubkey: &Pubkey, // must match create_address_with_seed(base, seed, program_id)
    base: &Pubkey,
    seed: &str,
    lamports: u64,
    space: u64,
    program_id: &Pubkey,
) -> Instruction {
    let account_metas = vec![
        AccountMeta::new(*from_pubkey, true),
        AccountMeta::new(*to_pubkey, false),
    ]
    .with_signer(base);

    Instruction::new(
        system_program::id(),
        &SystemInstruction::CreateAccountWithSeed {
            base: *base,
            seed: seed.to_string(),
            lamports,
            space,
            program_id: *program_id,
        },
        account_metas,
    )
}

pub fn assign(pubkey: &Pubkey, program_id: &Pubkey) -> Instruction {
    let account_metas = vec![AccountMeta::new(*pubkey, true)];
    Instruction::new(
        system_program::id(),
        &SystemInstruction::Assign {
            program_id: *program_id,
        },
        account_metas,
    )
}

pub fn assign_with_seed(
    address: &Pubkey, // must match create_address_with_seed(base, seed, program_id)
    base: &Pubkey,
    seed: &str,
    program_id: &Pubkey,
) -> Instruction {
    let account_metas = vec![AccountMeta::new(*address, false)].with_signer(base);
    Instruction::new(
        system_program::id(),
        &SystemInstruction::AssignWithSeed {
            base: *base,
            seed: seed.to_string(),
            program_id: *program_id,
        },
        account_metas,
    )
}

pub fn transfer(from_pubkey: &Pubkey, to_pubkey: &Pubkey, lamports: u64) -> Instruction {
    let account_metas = vec![
        AccountMeta::new(*from_pubkey, true),
        AccountMeta::new(*to_pubkey, false),
    ];
    Instruction::new(
        system_program::id(),
        &SystemInstruction::Transfer { lamports },
        account_metas,
    )
}

pub fn allocate(pubkey: &Pubkey, space: u64) -> Instruction {
    let account_metas = vec![AccountMeta::new(*pubkey, true)];
    Instruction::new(
        system_program::id(),
        &SystemInstruction::Allocate { space },
        account_metas,
    )
}

pub fn allocate_with_seed(
    address: &Pubkey, // must match create_address_with_seed(base, seed, program_id)
    base: &Pubkey,
    seed: &str,
    space: u64,
    program_id: &Pubkey,
) -> Instruction {
    let account_metas = vec![AccountMeta::new(*address, false)].with_signer(base);
    Instruction::new(
        system_program::id(),
        &SystemInstruction::AllocateWithSeed {
            base: *base,
            seed: seed.to_string(),
            space,
            program_id: *program_id,
        },
        account_metas,
    )
}

/// Create and sign new SystemInstruction::Transfer transaction to many destinations
pub fn transfer_many(from_pubkey: &Pubkey, to_lamports: &[(Pubkey, u64)]) -> Vec<Instruction> {
    to_lamports
        .iter()
        .map(|(to_pubkey, lamports)| transfer(from_pubkey, to_pubkey, *lamports))
        .collect()
}

pub fn create_address_with_seed(
    base: &Pubkey,
    seed: &str,
    program_id: &Pubkey,
) -> Result<Pubkey, SystemError> {
    if seed.len() > MAX_ADDRESS_SEED_LEN {
        return Err(SystemError::MaxSeedLengthExceeded);
    }

    Ok(Pubkey::new(
        hashv(&[base.as_ref(), seed.as_ref(), program_id.as_ref()]).as_ref(),
    ))
}

pub fn create_nonce_account_with_seed(
    from_pubkey: &Pubkey,
    nonce_pubkey: &Pubkey,
    base: &Pubkey,
    seed: &str,
    authority: &Pubkey,
    lamports: u64,
) -> Vec<Instruction> {
    vec![
        create_account_with_seed(
            from_pubkey,
            nonce_pubkey,
            base,
            seed,
            lamports,
            NonceState::size() as u64,
            &system_program::id(),
        ),
        Instruction::new(
            system_program::id(),
            &SystemInstruction::InitializeNonceAccount(*authority),
            vec![
                AccountMeta::new(*nonce_pubkey, false),
                AccountMeta::new_readonly(recent_blockhashes::id(), false),
                AccountMeta::new_readonly(rent::id(), false),
            ],
        ),
    ]
}

pub fn create_nonce_account(
    from_pubkey: &Pubkey,
    nonce_pubkey: &Pubkey,
    authority: &Pubkey,
    lamports: u64,
) -> Vec<Instruction> {
    vec![
        create_account(
            from_pubkey,
            nonce_pubkey,
            lamports,
            NonceState::size() as u64,
            &system_program::id(),
        ),
        Instruction::new(
            system_program::id(),
            &SystemInstruction::InitializeNonceAccount(*authority),
            vec![
                AccountMeta::new(*nonce_pubkey, false),
                AccountMeta::new_readonly(recent_blockhashes::id(), false),
                AccountMeta::new_readonly(rent::id(), false),
            ],
        ),
    ]
}

pub fn advance_nonce_account(nonce_pubkey: &Pubkey, authorized_pubkey: &Pubkey) -> Instruction {
    let account_metas = vec![
        AccountMeta::new(*nonce_pubkey, false),
        AccountMeta::new_readonly(recent_blockhashes::id(), false),
    ]
    .with_signer(authorized_pubkey);
    Instruction::new(
        system_program::id(),
        &SystemInstruction::AdvanceNonceAccount,
        account_metas,
    )
}

pub fn withdraw_nonce_account(
    nonce_pubkey: &Pubkey,
    authorized_pubkey: &Pubkey,
    to_pubkey: &Pubkey,
    lamports: u64,
) -> Instruction {
    let account_metas = vec![
        AccountMeta::new(*nonce_pubkey, false),
        AccountMeta::new(*to_pubkey, false),
        AccountMeta::new_readonly(recent_blockhashes::id(), false),
        AccountMeta::new_readonly(rent::id(), false),
    ]
    .with_signer(authorized_pubkey);
    Instruction::new(
        system_program::id(),
        &SystemInstruction::WithdrawNonceAccount(lamports),
        account_metas,
    )
}

pub fn authorize_nonce_account(
    nonce_pubkey: &Pubkey,
    authorized_pubkey: &Pubkey,
    new_authority: &Pubkey,
) -> Instruction {
    let account_metas = vec![AccountMeta::new(*nonce_pubkey, false)].with_signer(authorized_pubkey);
    Instruction::new(
        system_program::id(),
        &SystemInstruction::AuthorizeNonceAccount(*new_authority),
        account_metas,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instruction::{Instruction, InstructionError};

    fn get_keys(instruction: &Instruction) -> Vec<Pubkey> {
        instruction.accounts.iter().map(|x| x.pubkey).collect()
    }

    #[test]
    fn test_create_address_with_seed() {
        assert!(create_address_with_seed(&Pubkey::new_rand(), "☉", &Pubkey::new_rand()).is_ok());
        assert_eq!(
            create_address_with_seed(
                &Pubkey::new_rand(),
                std::str::from_utf8(&[127; MAX_ADDRESS_SEED_LEN + 1]).unwrap(),
                &Pubkey::new_rand()
            ),
            Err(SystemError::MaxSeedLengthExceeded)
        );
        assert!(create_address_with_seed(
            &Pubkey::new_rand(),
            "\
             \u{10FFFF}\u{10FFFF}\u{10FFFF}\u{10FFFF}\u{10FFFF}\u{10FFFF}\u{10FFFF}\u{10FFFF}\
             ",
            &Pubkey::new_rand()
        )
        .is_ok());
        // utf-8 abuse ;)
        assert_eq!(
            create_address_with_seed(
                &Pubkey::new_rand(),
                "\
                 x\u{10FFFF}\u{10FFFF}\u{10FFFF}\u{10FFFF}\u{10FFFF}\u{10FFFF}\u{10FFFF}\u{10FFFF}\
                 ",
                &Pubkey::new_rand()
            ),
            Err(SystemError::MaxSeedLengthExceeded)
        );

        assert!(create_address_with_seed(
            &Pubkey::new_rand(),
            std::str::from_utf8(&[0; MAX_ADDRESS_SEED_LEN]).unwrap(),
            &Pubkey::new_rand(),
        )
        .is_ok());

        assert!(create_address_with_seed(&Pubkey::new_rand(), "", &Pubkey::new_rand(),).is_ok());

        assert_eq!(
            create_address_with_seed(
                &Pubkey::default(),
                "limber chicken: 4/45",
                &Pubkey::default(),
            ),
            Ok("9h1HyLCW5dZnBVap8C5egQ9Z6pHyjsh5MNy83iPqqRuq"
                .parse()
                .unwrap())
        );
    }

    #[test]
    fn test_move_many() {
        let alice_pubkey = Pubkey::new_rand();
        let bob_pubkey = Pubkey::new_rand();
        let carol_pubkey = Pubkey::new_rand();
        let to_lamports = vec![(bob_pubkey, 1), (carol_pubkey, 2)];

        let instructions = transfer_many(&alice_pubkey, &to_lamports);
        assert_eq!(instructions.len(), 2);
        assert_eq!(get_keys(&instructions[0]), vec![alice_pubkey, bob_pubkey]);
        assert_eq!(get_keys(&instructions[1]), vec![alice_pubkey, carol_pubkey]);
    }

    #[test]
    fn test_create_nonce_account() {
        let from_pubkey = Pubkey::new_rand();
        let nonce_pubkey = Pubkey::new_rand();
        let authorized = nonce_pubkey;
        let ixs = create_nonce_account(&from_pubkey, &nonce_pubkey, &authorized, 42);
        assert_eq!(ixs.len(), 2);
        let ix = &ixs[0];
        assert_eq!(ix.program_id, system_program::id());
        let pubkeys: Vec<_> = ix.accounts.iter().map(|am| am.pubkey).collect();
        assert!(pubkeys.contains(&from_pubkey));
        assert!(pubkeys.contains(&nonce_pubkey));
    }

    #[test]
    fn test_nonce_error_decode() {
        use num_traits::FromPrimitive;
        fn pretty_err<T>(err: InstructionError) -> String
        where
            T: 'static + std::error::Error + DecodeError<T> + FromPrimitive,
        {
            if let InstructionError::CustomError(code) = err {
                let specific_error: T = T::decode_custom_error_to_enum(code).unwrap();
                format!(
                    "{:?}: {}::{:?} - {}",
                    err,
                    T::type_of(),
                    specific_error,
                    specific_error,
                )
            } else {
                "".to_string()
            }
        }
        assert_eq!(
            "CustomError(0): NonceError::NoRecentBlockhashes - recent blockhash list is empty",
            pretty_err::<NonceError>(NonceError::NoRecentBlockhashes.into())
        );
        assert_eq!(
            "CustomError(1): NonceError::NotExpired - stored nonce is still in recent_blockhashes",
            pretty_err::<NonceError>(NonceError::NotExpired.into())
        );
        assert_eq!(
            "CustomError(2): NonceError::UnexpectedValue - specified nonce does not match stored nonce",
            pretty_err::<NonceError>(NonceError::UnexpectedValue.into())
        );
        assert_eq!(
            "CustomError(3): NonceError::BadAccountState - cannot handle request in current account state",
            pretty_err::<NonceError>(NonceError::BadAccountState.into())
        );
    }
}
