use crate::hash::hashv;
use crate::instruction::{AccountMeta, Instruction};
use crate::instruction_processor_utils::DecodeError;
use crate::pubkey::Pubkey;
use crate::system_program;
use num_derive::{FromPrimitive, ToPrimitive};

#[derive(Serialize, Debug, Clone, PartialEq, FromPrimitive, ToPrimitive)]
pub enum SystemError {
    AccountAlreadyInUse,
    ResultWithNegativeLamports,
    InvalidProgramId,
    InvalidAccountId,
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
    /// * space - memory to allocate if greater then zero
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
    ///    the from account and a seed
    /// * Transaction::keys[0] - source
    /// * Transaction::keys[1] - new account key
    /// * seed - string of ascii chars, no longer than MAX_ADDRESS_SEED_LEN
    /// * lamports - number of lamports to transfer to the new account
    /// * space - memory to allocate if greater then zero
    /// * program_id - the program id of the new account
    CreateAccountWithSeed {
        base: Pubkey,
        seed: String,
        lamports: u64,
        space: u64,
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

pub fn create_account_with_seed(
    from_pubkey: &Pubkey,
    to_pubkey: &Pubkey,
    base: &Pubkey,
    seed: &str,
    lamports: u64,
    space: u64,
    program_id: &Pubkey,
) -> Instruction {
    let account_metas = vec![
        AccountMeta::new(*from_pubkey, true),
        AccountMeta::new(*to_pubkey, false),
    ];

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

pub fn assign(from_pubkey: &Pubkey, program_id: &Pubkey) -> Instruction {
    let account_metas = vec![AccountMeta::new(*from_pubkey, true)];
    Instruction::new(
        system_program::id(),
        &SystemInstruction::Assign {
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

/// Create and sign new SystemInstruction::Transfer transaction to many destinations
pub fn transfer_many(from_pubkey: &Pubkey, to_lamports: &[(Pubkey, u64)]) -> Vec<Instruction> {
    to_lamports
        .iter()
        .map(|(to_pubkey, lamports)| transfer(from_pubkey, to_pubkey, *lamports))
        .collect()
}

pub fn create_address_with_seed(
    from_pubkey: &Pubkey,
    seed: &str,
    program_id: &Pubkey,
) -> Result<Pubkey, SystemError> {
    if seed.len() > MAX_ADDRESS_SEED_LEN {
        return Err(SystemError::MaxSeedLengthExceeded);
    }

    Ok(Pubkey::new(
        hashv(&[from_pubkey.as_ref(), seed.as_ref(), program_id.as_ref()]).as_ref(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
