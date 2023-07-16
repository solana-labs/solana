//! Instructions and constructors for the system program.
//!
//! The system program is responsible for the creation of accounts and [nonce
//! accounts][na]. It is responsible for transferring lamports from accounts
//! owned by the system program, including typical user wallet accounts.
//!
//! [na]: https://docs.solana.com/implemented-proposals/durable-tx-nonces
//!
//! Account creation typically involves three steps: [`allocate`] space,
//! [`transfer`] lamports for rent, [`assign`] to its owning program. The
//! [`create_account`] function does all three at once. All new accounts must
//! contain enough lamports to be [rent exempt], or else the creation
//! instruction will fail.
//!
//! [rent exempt]: https://docs.solana.com/developing/programming-model/accounts#rent-exemption
//!
//! The accounts created by the system program can either be user-controlled,
//! where the secret keys are held outside the blockchain,
//! or they can be [program derived addresses][pda],
//! where write access to accounts is granted by an owning program.
//!
//! [pda]: crate::pubkey::Pubkey::find_program_address
//!
//! The system program ID is defined in [`system_program`].
//!
//! Most of the functions in this module construct an [`Instruction`], that must
//! be submitted to the runtime for execution, either via RPC, typically with
//! [`RpcClient`], or through [cross-program invocation][cpi].
//!
//! When invoking through CPI, the [`invoke`] or [`invoke_signed`] instruction
//! requires all account references to be provided explicitly as [`AccountInfo`]
//! values. The account references required are specified in the documentation
//! for the [`SystemInstruction`] variants for each system program instruction,
//! and these variants are linked from the documentation for their constructors.
//!
//! [`RpcClient`]: https://docs.rs/solana-client/latest/solana_client/rpc_client/struct.RpcClient.html
//! [cpi]: crate::program
//! [`invoke`]: crate::program::invoke
//! [`invoke_signed`]: crate::program::invoke_signed
//! [`AccountInfo`]: crate::account_info::AccountInfo

#[allow(deprecated)]
use {
    crate::{
        decode_error::DecodeError,
        instruction::{AccountMeta, Instruction},
        nonce,
        pubkey::Pubkey,
        system_program,
        sysvar::{recent_blockhashes, rent},
    },
    num_derive::{FromPrimitive, ToPrimitive},
    thiserror::Error,
};

#[derive(Error, Debug, Serialize, Clone, PartialEq, Eq, FromPrimitive, ToPrimitive)]
pub enum SystemError {
    #[error("an account with the same address already exists")]
    AccountAlreadyInUse,
    #[error("account does not have enough SOL to perform the operation")]
    ResultWithNegativeLamports,
    #[error("cannot assign account to this program id")]
    InvalidProgramId,
    #[error("cannot allocate account data of this length")]
    InvalidAccountDataLength,
    #[error("length of requested seed is too long")]
    MaxSeedLengthExceeded,
    #[error("provided address does not match addressed derived from seed")]
    AddressWithSeedMismatch,
    #[error("advancing stored nonce requires a populated RecentBlockhashes sysvar")]
    NonceNoRecentBlockhashes,
    #[error("stored nonce is still in recent_blockhashes")]
    NonceBlockhashNotExpired,
    #[error("specified nonce does not match stored nonce")]
    NonceUnexpectedBlockhashValue,
}

impl<T> DecodeError<T> for SystemError {
    fn type_of() -> &'static str {
        "SystemError"
    }
}

/// Maximum permitted size of account data (10 MiB).
pub const MAX_PERMITTED_DATA_LENGTH: u64 = 10 * 1024 * 1024;

/// Maximum permitted size of new allocations per transaction, in bytes.
///
/// The value was chosen such that at least one max sized account could be created,
/// plus some additional resize allocations.
pub const MAX_PERMITTED_ACCOUNTS_DATA_ALLOCATIONS_PER_TRANSACTION: i64 =
    MAX_PERMITTED_DATA_LENGTH as i64 * 2;

// SBF program entrypoint assumes that the max account data length
// will fit inside a u32. If this constant no longer fits in a u32,
// the entrypoint deserialization code in the SDK must be updated.
#[cfg(test)]
static_assertions::const_assert!(MAX_PERMITTED_DATA_LENGTH <= u32::MAX as u64);

#[cfg(test)]
static_assertions::const_assert_eq!(MAX_PERMITTED_DATA_LENGTH, 10_485_760);

/// An instruction to the system program.
#[frozen_abi(digest = "5e22s2kFu9Do77hdcCyxyhuKHD8ThAB6Q6dNaLTCjL5M")]
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, AbiExample, AbiEnumVisitor)]
pub enum SystemInstruction {
    /// Create a new account
    ///
    /// # Account references
    ///   0. `[WRITE, SIGNER]` Funding account
    ///   1. `[WRITE, SIGNER]` New account
    CreateAccount {
        /// Number of lamports to transfer to the new account
        lamports: u64,

        /// Number of bytes of memory to allocate
        space: u64,

        /// Address of program that will own the new account
        owner: Pubkey,
    },

    /// Assign account to a program
    ///
    /// # Account references
    ///   0. `[WRITE, SIGNER]` Assigned account public key
    Assign {
        /// Owner program account
        owner: Pubkey,
    },

    /// Transfer lamports
    ///
    /// # Account references
    ///   0. `[WRITE, SIGNER]` Funding account
    ///   1. `[WRITE]` Recipient account
    Transfer { lamports: u64 },

    /// Create a new account at an address derived from a base pubkey and a seed
    ///
    /// # Account references
    ///   0. `[WRITE, SIGNER]` Funding account
    ///   1. `[WRITE]` Created account
    ///   2. `[SIGNER]` (optional) Base account; the account matching the base Pubkey below must be
    ///                          provided as a signer, but may be the same as the funding account
    ///                          and provided as account 0
    CreateAccountWithSeed {
        /// Base public key
        base: Pubkey,

        /// String of ASCII chars, no longer than `Pubkey::MAX_SEED_LEN`
        seed: String,

        /// Number of lamports to transfer to the new account
        lamports: u64,

        /// Number of bytes of memory to allocate
        space: u64,

        /// Owner program account address
        owner: Pubkey,
    },

    /// Consumes a stored nonce, replacing it with a successor
    ///
    /// # Account references
    ///   0. `[WRITE]` Nonce account
    ///   1. `[]` RecentBlockhashes sysvar
    ///   2. `[SIGNER]` Nonce authority
    AdvanceNonceAccount,

    /// Withdraw funds from a nonce account
    ///
    /// # Account references
    ///   0. `[WRITE]` Nonce account
    ///   1. `[WRITE]` Recipient account
    ///   2. `[]` RecentBlockhashes sysvar
    ///   3. `[]` Rent sysvar
    ///   4. `[SIGNER]` Nonce authority
    ///
    /// The `u64` parameter is the lamports to withdraw, which must leave the
    /// account balance above the rent exempt reserve or at zero.
    WithdrawNonceAccount(u64),

    /// Drive state of Uninitialized nonce account to Initialized, setting the nonce value
    ///
    /// # Account references
    ///   0. `[WRITE]` Nonce account
    ///   1. `[]` RecentBlockhashes sysvar
    ///   2. `[]` Rent sysvar
    ///
    /// The `Pubkey` parameter specifies the entity authorized to execute nonce
    /// instruction on the account
    ///
    /// No signatures are required to execute this instruction, enabling derived
    /// nonce account addresses
    InitializeNonceAccount(Pubkey),

    /// Change the entity authorized to execute nonce instructions on the account
    ///
    /// # Account references
    ///   0. `[WRITE]` Nonce account
    ///   1. `[SIGNER]` Nonce authority
    ///
    /// The `Pubkey` parameter identifies the entity to authorize
    AuthorizeNonceAccount(Pubkey),

    /// Allocate space in a (possibly new) account without funding
    ///
    /// # Account references
    ///   0. `[WRITE, SIGNER]` New account
    Allocate {
        /// Number of bytes of memory to allocate
        space: u64,
    },

    /// Allocate space for and assign an account at an address
    ///    derived from a base public key and a seed
    ///
    /// # Account references
    ///   0. `[WRITE]` Allocated account
    ///   1. `[SIGNER]` Base account
    AllocateWithSeed {
        /// Base public key
        base: Pubkey,

        /// String of ASCII chars, no longer than `pubkey::MAX_SEED_LEN`
        seed: String,

        /// Number of bytes of memory to allocate
        space: u64,

        /// Owner program account
        owner: Pubkey,
    },

    /// Assign account to a program based on a seed
    ///
    /// # Account references
    ///   0. `[WRITE]` Assigned account
    ///   1. `[SIGNER]` Base account
    AssignWithSeed {
        /// Base public key
        base: Pubkey,

        /// String of ASCII chars, no longer than `pubkey::MAX_SEED_LEN`
        seed: String,

        /// Owner program account
        owner: Pubkey,
    },

    /// Transfer lamports from a derived address
    ///
    /// # Account references
    ///   0. `[WRITE]` Funding account
    ///   1. `[SIGNER]` Base for funding account
    ///   2. `[WRITE]` Recipient account
    TransferWithSeed {
        /// Amount to transfer
        lamports: u64,

        /// Seed to use to derive the funding account address
        from_seed: String,

        /// Owner to use to derive the funding account address
        from_owner: Pubkey,
    },

    /// One-time idempotent upgrade of legacy nonce versions in order to bump
    /// them out of chain blockhash domain.
    ///
    /// # Account references
    ///   0. `[WRITE]` Nonce account
    UpgradeNonceAccount,
}

/// Create an account.
///
/// This function produces an [`Instruction`] which must be submitted in a
/// [`Transaction`] or [invoked] to take effect, containing a serialized
/// [`SystemInstruction::CreateAccount`].
///
/// [`Transaction`]: https://docs.rs/solana-sdk/latest/solana_sdk/transaction/struct.Transaction.html
/// [invoked]: crate::program::invoke
///
/// Account creation typically involves three steps: [`allocate`] space,
/// [`transfer`] lamports for rent, [`assign`] to its owning program. The
/// [`create_account`] function does all three at once.
///
/// # Required signers
///
/// The `from_pubkey` and `to_pubkey` signers must sign the transaction.
///
/// # Examples
///
/// These examples use a single invocation of
/// [`SystemInstruction::CreateAccount`] to create a new account, allocate some
/// space, transfer it the minimum lamports for rent exemption, and assign it to
/// the system program,
///
/// ## Example: client-side RPC
///
/// This example submits the instruction from an RPC client.
/// The `payer` and `new_account` are signers.
///
/// ```
/// # use solana_program::example_mocks::{solana_sdk, solana_rpc_client};
/// use solana_rpc_client::rpc_client::RpcClient;
/// use solana_sdk::{
///     pubkey::Pubkey,
///     signature::{Keypair, Signer},
///     system_instruction,
///     system_program,
///     transaction::Transaction,
/// };
/// use anyhow::Result;
///
/// fn create_account(
///     client: &RpcClient,
///     payer: &Keypair,
///     new_account: &Keypair,
///     space: u64,
/// ) -> Result<()> {
///     let rent = client.get_minimum_balance_for_rent_exemption(space.try_into()?)?;
///     let instr = system_instruction::create_account(
///         &payer.pubkey(),
///         &new_account.pubkey(),
///         rent,
///         space,
///         &system_program::ID,
///     );
///
///     let blockhash = client.get_latest_blockhash()?;
///     let tx = Transaction::new_signed_with_payer(
///         &[instr],
///         Some(&payer.pubkey()),
///         &[payer, new_account],
///         blockhash,
///     );
///
///     let _sig = client.send_and_confirm_transaction(&tx)?;
///
///     Ok(())
/// }
/// # let payer = Keypair::new();
/// # let new_account = Keypair::new();
/// # let client = RpcClient::new(String::new());
/// # create_account(&client, &payer, &new_account, 0);
/// #
/// # Ok::<(), anyhow::Error>(())
/// ```
///
/// ## Example: on-chain program
///
/// This example submits the instruction from an on-chain Solana program. The
/// created account is a [program derived address][pda]. The `payer` and
/// `new_account_pda` are signers, with `new_account_pda` being signed for
/// virtually by the program itself via [`invoke_signed`], `payer` being signed
/// for by the client that submitted the transaction.
///
/// [pda]: Pubkey::find_program_address
/// [`invoke_signed`]: crate::program::invoke_signed
///
/// ```
/// # use borsh::{BorshDeserialize, BorshSerialize};
/// use solana_program::{
///     account_info::{next_account_info, AccountInfo},
///     entrypoint,
///     entrypoint::ProgramResult,
///     msg,
///     program::invoke_signed,
///     pubkey::Pubkey,
///     system_instruction,
///     system_program,
///     sysvar::rent::Rent,
///     sysvar::Sysvar,
/// };
///
/// #[derive(BorshSerialize, BorshDeserialize, Debug)]
/// pub struct CreateAccountInstruction {
///     /// The PDA seed used to distinguish the new account from other PDAs
///     pub new_account_seed: [u8; 16],
///     /// The PDA bump seed
///     pub new_account_bump_seed: u8,
///     /// The amount of space to allocate for `new_account_pda`
///     pub space: u64,
/// }
///
/// entrypoint!(process_instruction);
///
/// fn process_instruction(
///     program_id: &Pubkey,
///     accounts: &[AccountInfo],
///     instruction_data: &[u8],
/// ) -> ProgramResult {
///     let instr = CreateAccountInstruction::deserialize(&mut &instruction_data[..])?;
///
///     let account_info_iter = &mut accounts.iter();
///
///     let payer = next_account_info(account_info_iter)?;
///     let new_account_pda = next_account_info(account_info_iter)?;
///     let system_account = next_account_info(account_info_iter)?;
///
///     assert!(payer.is_signer);
///     assert!(payer.is_writable);
///     // Note that `new_account_pda` is not a signer yet.
///     // This program will sign for it via `invoke_signed`.
///     assert!(!new_account_pda.is_signer);
///     assert!(new_account_pda.is_writable);
///     assert!(system_program::check_id(system_account.key));
///
///     let new_account_seed = &instr.new_account_seed;
///     let new_account_bump_seed = instr.new_account_bump_seed;
///
///     let rent = Rent::get()?
///         .minimum_balance(instr.space.try_into().expect("overflow"));
///
///     invoke_signed(
///         &system_instruction::create_account(
///             payer.key,
///             new_account_pda.key,
///             rent,
///             instr.space,
///             &system_program::ID
///         ),
///         &[payer.clone(), new_account_pda.clone()],
///         &[&[payer.key.as_ref(), new_account_seed, &[new_account_bump_seed]]],
///     )?;
///
///     Ok(())
/// }
///
/// # Ok::<(), anyhow::Error>(())
/// ```
pub fn create_account(
    from_pubkey: &Pubkey,
    to_pubkey: &Pubkey,
    lamports: u64,
    space: u64,
    owner: &Pubkey,
) -> Instruction {
    let account_metas = vec![
        AccountMeta::new(*from_pubkey, true),
        AccountMeta::new(*to_pubkey, true),
    ];
    Instruction::new_with_bincode(
        system_program::id(),
        &SystemInstruction::CreateAccount {
            lamports,
            space,
            owner: *owner,
        },
        account_metas,
    )
}

// we accept `to` as a parameter so that callers do their own error handling when
//   calling create_with_seed()
pub fn create_account_with_seed(
    from_pubkey: &Pubkey,
    to_pubkey: &Pubkey, // must match create_with_seed(base, seed, owner)
    base: &Pubkey,
    seed: &str,
    lamports: u64,
    space: u64,
    owner: &Pubkey,
) -> Instruction {
    let account_metas = vec![
        AccountMeta::new(*from_pubkey, true),
        AccountMeta::new(*to_pubkey, false),
        AccountMeta::new_readonly(*base, true),
    ];

    Instruction::new_with_bincode(
        system_program::id(),
        &SystemInstruction::CreateAccountWithSeed {
            base: *base,
            seed: seed.to_string(),
            lamports,
            space,
            owner: *owner,
        },
        account_metas,
    )
}

/// Assign ownership of an account from the system program.
///
/// This function produces an [`Instruction`] which must be submitted in a
/// [`Transaction`] or [invoked] to take effect, containing a serialized
/// [`SystemInstruction::Assign`].
///
/// [`Transaction`]: https://docs.rs/solana-sdk/latest/solana_sdk/transaction/struct.Transaction.html
/// [invoked]: crate::program::invoke
///
/// # Required signers
///
/// The `pubkey` signer must sign the transaction.
///
/// # Examples
///
/// These examples allocate space for an account, transfer it the minimum
/// balance for rent exemption, and assign the account to a program.
///
/// ## Example: client-side RPC
///
/// This example submits the instructions from an RPC client.
/// It assigns the account to a provided program account.
/// The `payer` and `new_account` are signers.
///
/// ```
/// # use solana_program::example_mocks::{solana_sdk, solana_rpc_client};
/// use solana_rpc_client::rpc_client::RpcClient;
/// use solana_sdk::{
///     pubkey::Pubkey,
///     signature::{Keypair, Signer},
///     system_instruction,
///     transaction::Transaction,
/// };
/// use anyhow::Result;
///
/// fn create_account(
///     client: &RpcClient,
///     payer: &Keypair,
///     new_account: &Keypair,
///     owning_program: &Pubkey,
///     space: u64,
/// ) -> Result<()> {
///     let rent = client.get_minimum_balance_for_rent_exemption(space.try_into()?)?;
///
///     let transfer_instr = system_instruction::transfer(
///         &payer.pubkey(),
///         &new_account.pubkey(),
///         rent,
///     );
///
///     let allocate_instr = system_instruction::allocate(
///         &new_account.pubkey(),
///         space,
///     );
///
///     let assign_instr = system_instruction::assign(
///         &new_account.pubkey(),
///         owning_program,
///     );
///
///     let blockhash = client.get_latest_blockhash()?;
///     let tx = Transaction::new_signed_with_payer(
///         &[transfer_instr, allocate_instr, assign_instr],
///         Some(&payer.pubkey()),
///         &[payer, new_account],
///         blockhash,
///     );
///
///     let _sig = client.send_and_confirm_transaction(&tx)?;
///
///     Ok(())
/// }
/// # let client = RpcClient::new(String::new());
/// # let payer = Keypair::new();
/// # let new_account = Keypair::new();
/// # let owning_program = Pubkey::new_unique();
/// # create_account(&client, &payer, &new_account, &owning_program, 1);
/// #
/// # Ok::<(), anyhow::Error>(())
/// ```
///
/// ## Example: on-chain program
///
/// This example submits the instructions from an on-chain Solana program. The
/// created account is a [program derived address][pda], funded by `payer`, and
/// assigned to the running program. The `payer` and `new_account_pda` are
/// signers, with `new_account_pda` being signed for virtually by the program
/// itself via [`invoke_signed`], `payer` being signed for by the client that
/// submitted the transaction.
///
/// [pda]: Pubkey::find_program_address
/// [`invoke_signed`]: crate::program::invoke_signed
///
/// ```
/// # use borsh::{BorshDeserialize, BorshSerialize};
/// use solana_program::{
///     account_info::{next_account_info, AccountInfo},
///     entrypoint,
///     entrypoint::ProgramResult,
///     msg,
///     program::invoke_signed,
///     pubkey::Pubkey,
///     system_instruction,
///     system_program,
///     sysvar::rent::Rent,
///     sysvar::Sysvar,
/// };
///
/// #[derive(BorshSerialize, BorshDeserialize, Debug)]
/// pub struct CreateAccountInstruction {
///     /// The PDA seed used to distinguish the new account from other PDAs
///     pub new_account_seed: [u8; 16],
///     /// The PDA bump seed
///     pub new_account_bump_seed: u8,
///     /// The amount of space to allocate for `new_account_pda`
///     pub space: u64,
/// }
///
/// entrypoint!(process_instruction);
///
/// fn process_instruction(
///     program_id: &Pubkey,
///     accounts: &[AccountInfo],
///     instruction_data: &[u8],
/// ) -> ProgramResult {
///     let instr = CreateAccountInstruction::deserialize(&mut &instruction_data[..])?;
///
///     let account_info_iter = &mut accounts.iter();
///
///     let payer = next_account_info(account_info_iter)?;
///     let new_account_pda = next_account_info(account_info_iter)?;
///     let system_account = next_account_info(account_info_iter)?;
///
///     assert!(payer.is_signer);
///     assert!(payer.is_writable);
///     // Note that `new_account_pda` is not a signer yet.
///     // This program will sign for it via `invoke_signed`.
///     assert!(!new_account_pda.is_signer);
///     assert!(new_account_pda.is_writable);
///     assert!(system_program::check_id(system_account.key));
///
///     let new_account_seed = &instr.new_account_seed;
///     let new_account_bump_seed = instr.new_account_bump_seed;
///
///     let rent = Rent::get()?
///         .minimum_balance(instr.space.try_into().expect("overflow"));
///
///     invoke_signed(
///         &system_instruction::transfer(
///             payer.key,
///             new_account_pda.key,
///             rent,
///         ),
///         &[payer.clone(), new_account_pda.clone()],
///         &[&[payer.key.as_ref(), new_account_seed, &[new_account_bump_seed]]],
///     )?;
///
///     invoke_signed(
///         &system_instruction::allocate(
///             new_account_pda.key,
///             instr.space,
///         ),
///         &[new_account_pda.clone()],
///         &[&[payer.key.as_ref(), new_account_seed, &[new_account_bump_seed]]],
///     )?;
///
///     invoke_signed(
///         &system_instruction::assign(
///             new_account_pda.key,
///             &program_id,
///         ),
///         &[new_account_pda.clone()],
///         &[&[payer.key.as_ref(), new_account_seed, &[new_account_bump_seed]]],
///     )?;
///
///     Ok(())
/// }
///
/// # Ok::<(), anyhow::Error>(())
/// ```
pub fn assign(pubkey: &Pubkey, owner: &Pubkey) -> Instruction {
    let account_metas = vec![AccountMeta::new(*pubkey, true)];
    Instruction::new_with_bincode(
        system_program::id(),
        &SystemInstruction::Assign { owner: *owner },
        account_metas,
    )
}

pub fn assign_with_seed(
    address: &Pubkey, // must match create_with_seed(base, seed, owner)
    base: &Pubkey,
    seed: &str,
    owner: &Pubkey,
) -> Instruction {
    let account_metas = vec![
        AccountMeta::new(*address, false),
        AccountMeta::new_readonly(*base, true),
    ];
    Instruction::new_with_bincode(
        system_program::id(),
        &SystemInstruction::AssignWithSeed {
            base: *base,
            seed: seed.to_string(),
            owner: *owner,
        },
        account_metas,
    )
}

/// Transfer lamports from an account owned by the system program.
///
/// This function produces an [`Instruction`] which must be submitted in a
/// [`Transaction`] or [invoked] to take effect, containing a serialized
/// [`SystemInstruction::Transfer`].
///
/// [`Transaction`]: https://docs.rs/solana-sdk/latest/solana_sdk/transaction/struct.Transaction.html
/// [invoked]: crate::program::invoke
///
/// # Required signers
///
/// The `from_pubkey` signer must sign the transaction.
///
/// # Examples
///
/// These examples allocate space for an account, transfer it the minimum
/// balance for rent exemption, and assign the account to a program.
///
/// # Example: client-side RPC
///
/// This example submits the instructions from an RPC client.
/// It assigns the account to a provided program account.
/// The `payer` and `new_account` are signers.
///
/// ```
/// # use solana_program::example_mocks::{solana_sdk, solana_rpc_client};
/// use solana_rpc_client::rpc_client::RpcClient;
/// use solana_sdk::{
///     pubkey::Pubkey,
///     signature::{Keypair, Signer},
///     system_instruction,
///     transaction::Transaction,
/// };
/// use anyhow::Result;
///
/// fn create_account(
///     client: &RpcClient,
///     payer: &Keypair,
///     new_account: &Keypair,
///     owning_program: &Pubkey,
///     space: u64,
/// ) -> Result<()> {
///     let rent = client.get_minimum_balance_for_rent_exemption(space.try_into()?)?;
///
///     let transfer_instr = system_instruction::transfer(
///         &payer.pubkey(),
///         &new_account.pubkey(),
///         rent,
///     );
///
///     let allocate_instr = system_instruction::allocate(
///         &new_account.pubkey(),
///         space,
///     );
///
///     let assign_instr = system_instruction::assign(
///         &new_account.pubkey(),
///         owning_program,
///     );
///
///     let blockhash = client.get_latest_blockhash()?;
///     let tx = Transaction::new_signed_with_payer(
///         &[transfer_instr, allocate_instr, assign_instr],
///         Some(&payer.pubkey()),
///         &[payer, new_account],
///         blockhash,
///     );
///
///     let _sig = client.send_and_confirm_transaction(&tx)?;
///
///     Ok(())
/// }
/// # let client = RpcClient::new(String::new());
/// # let payer = Keypair::new();
/// # let new_account = Keypair::new();
/// # let owning_program = Pubkey::new_unique();
/// # create_account(&client, &payer, &new_account, &owning_program, 1);
/// #
/// # Ok::<(), anyhow::Error>(())
/// ```
///
/// ## Example: on-chain program
///
/// This example submits the instructions from an on-chain Solana program. The
/// created account is a [program derived address][pda], funded by `payer`, and
/// assigned to the running program. The `payer` and `new_account_pda` are
/// signers, with `new_account_pda` being signed for virtually by the program
/// itself via [`invoke_signed`], `payer` being signed for by the client that
/// submitted the transaction.
///
/// [pda]: Pubkey::find_program_address
/// [`invoke_signed`]: crate::program::invoke_signed
///
/// ```
/// # use borsh::{BorshDeserialize, BorshSerialize};
/// use solana_program::{
///     account_info::{next_account_info, AccountInfo},
///     entrypoint,
///     entrypoint::ProgramResult,
///     msg,
///     program::invoke_signed,
///     pubkey::Pubkey,
///     system_instruction,
///     system_program,
///     sysvar::rent::Rent,
///     sysvar::Sysvar,
/// };
///
/// #[derive(BorshSerialize, BorshDeserialize, Debug)]
/// pub struct CreateAccountInstruction {
///     /// The PDA seed used to distinguish the new account from other PDAs
///     pub new_account_seed: [u8; 16],
///     /// The PDA bump seed
///     pub new_account_bump_seed: u8,
///     /// The amount of space to allocate for `new_account_pda`
///     pub space: u64,
/// }
///
/// entrypoint!(process_instruction);
///
/// fn process_instruction(
///     program_id: &Pubkey,
///     accounts: &[AccountInfo],
///     instruction_data: &[u8],
/// ) -> ProgramResult {
///     let instr = CreateAccountInstruction::deserialize(&mut &instruction_data[..])?;
///
///     let account_info_iter = &mut accounts.iter();
///
///     let payer = next_account_info(account_info_iter)?;
///     let new_account_pda = next_account_info(account_info_iter)?;
///     let system_account = next_account_info(account_info_iter)?;
///
///     assert!(payer.is_signer);
///     assert!(payer.is_writable);
///     // Note that `new_account_pda` is not a signer yet.
///     // This program will sign for it via `invoke_signed`.
///     assert!(!new_account_pda.is_signer);
///     assert!(new_account_pda.is_writable);
///     assert!(system_program::check_id(system_account.key));
///
///     let new_account_seed = &instr.new_account_seed;
///     let new_account_bump_seed = instr.new_account_bump_seed;
///
///     let rent = Rent::get()?
///         .minimum_balance(instr.space.try_into().expect("overflow"));
///
///     invoke_signed(
///         &system_instruction::transfer(
///             payer.key,
///             new_account_pda.key,
///             rent,
///         ),
///         &[payer.clone(), new_account_pda.clone()],
///         &[&[payer.key.as_ref(), new_account_seed, &[new_account_bump_seed]]],
///     )?;
///
///     invoke_signed(
///         &system_instruction::allocate(
///             new_account_pda.key,
///             instr.space,
///         ),
///         &[new_account_pda.clone()],
///         &[&[payer.key.as_ref(), new_account_seed, &[new_account_bump_seed]]],
///     )?;
///
///     invoke_signed(
///         &system_instruction::assign(
///             new_account_pda.key,
///             &program_id,
///         ),
///         &[new_account_pda.clone()],
///         &[&[payer.key.as_ref(), new_account_seed, &[new_account_bump_seed]]],
///     )?;
///
///     Ok(())
/// }
///
/// # Ok::<(), anyhow::Error>(())
/// ```
pub fn transfer(from_pubkey: &Pubkey, to_pubkey: &Pubkey, lamports: u64) -> Instruction {
    let account_metas = vec![
        AccountMeta::new(*from_pubkey, true),
        AccountMeta::new(*to_pubkey, false),
    ];
    Instruction::new_with_bincode(
        system_program::id(),
        &SystemInstruction::Transfer { lamports },
        account_metas,
    )
}

pub fn transfer_with_seed(
    from_pubkey: &Pubkey, // must match create_with_seed(base, seed, owner)
    from_base: &Pubkey,
    from_seed: String,
    from_owner: &Pubkey,
    to_pubkey: &Pubkey,
    lamports: u64,
) -> Instruction {
    let account_metas = vec![
        AccountMeta::new(*from_pubkey, false),
        AccountMeta::new_readonly(*from_base, true),
        AccountMeta::new(*to_pubkey, false),
    ];
    Instruction::new_with_bincode(
        system_program::id(),
        &SystemInstruction::TransferWithSeed {
            lamports,
            from_seed,
            from_owner: *from_owner,
        },
        account_metas,
    )
}

/// Allocate space for an account.
///
/// This function produces an [`Instruction`] which must be submitted in a
/// [`Transaction`] or [invoked] to take effect, containing a serialized
/// [`SystemInstruction::Allocate`].
///
/// [`Transaction`]: https://docs.rs/solana-sdk/latest/solana_sdk/transaction/struct.Transaction.html
/// [invoked]: crate::program::invoke
///
/// The transaction will fail if the account already has size greater than 0,
/// or if the requested size is greater than [`MAX_PERMITTED_DATA_LENGTH`].
///
/// # Required signers
///
/// The `pubkey` signer must sign the transaction.
///
/// # Examples
///
/// These examples allocate space for an account, transfer it the minimum
/// balance for rent exemption, and assign the account to a program.
///
/// # Example: client-side RPC
///
/// This example submits the instructions from an RPC client.
/// It assigns the account to a provided program account.
/// The `payer` and `new_account` are signers.
///
/// ```
/// # use solana_program::example_mocks::{solana_sdk, solana_rpc_client};
/// use solana_rpc_client::rpc_client::RpcClient;
/// use solana_sdk::{
///     pubkey::Pubkey,
///     signature::{Keypair, Signer},
///     system_instruction,
///     transaction::Transaction,
/// };
/// use anyhow::Result;
///
/// fn create_account(
///     client: &RpcClient,
///     payer: &Keypair,
///     new_account: &Keypair,
///     owning_program: &Pubkey,
///     space: u64,
/// ) -> Result<()> {
///     let rent = client.get_minimum_balance_for_rent_exemption(space.try_into()?)?;
///
///     let transfer_instr = system_instruction::transfer(
///         &payer.pubkey(),
///         &new_account.pubkey(),
///         rent,
///     );
///
///     let allocate_instr = system_instruction::allocate(
///         &new_account.pubkey(),
///         space,
///     );
///
///     let assign_instr = system_instruction::assign(
///         &new_account.pubkey(),
///         owning_program,
///     );
///
///     let blockhash = client.get_latest_blockhash()?;
///     let tx = Transaction::new_signed_with_payer(
///         &[transfer_instr, allocate_instr, assign_instr],
///         Some(&payer.pubkey()),
///         &[payer, new_account],
///         blockhash,
///     );
///
///     let _sig = client.send_and_confirm_transaction(&tx)?;
///
///     Ok(())
/// }
/// # let client = RpcClient::new(String::new());
/// # let payer = Keypair::new();
/// # let new_account = Keypair::new();
/// # let owning_program = Pubkey::new_unique();
/// # create_account(&client, &payer, &new_account, &owning_program, 1);
/// #
/// # Ok::<(), anyhow::Error>(())
/// ```
///
/// ## Example: on-chain program
///
/// This example submits the instructions from an on-chain Solana program. The
/// created account is a [program derived address][pda], funded by `payer`, and
/// assigned to the running program. The `payer` and `new_account_pda` are
/// signers, with `new_account_pda` being signed for virtually by the program
/// itself via [`invoke_signed`], `payer` being signed for by the client that
/// submitted the transaction.
///
/// [pda]: Pubkey::find_program_address
/// [`invoke_signed`]: crate::program::invoke_signed
///
/// ```
/// # use borsh::{BorshDeserialize, BorshSerialize};
/// use solana_program::{
///     account_info::{next_account_info, AccountInfo},
///     entrypoint,
///     entrypoint::ProgramResult,
///     msg,
///     program::invoke_signed,
///     pubkey::Pubkey,
///     system_instruction,
///     system_program,
///     sysvar::rent::Rent,
///     sysvar::Sysvar,
/// };
///
/// #[derive(BorshSerialize, BorshDeserialize, Debug)]
/// pub struct CreateAccountInstruction {
///     /// The PDA seed used to distinguish the new account from other PDAs
///     pub new_account_seed: [u8; 16],
///     /// The PDA bump seed
///     pub new_account_bump_seed: u8,
///     /// The amount of space to allocate for `new_account_pda`
///     pub space: u64,
/// }
///
/// entrypoint!(process_instruction);
///
/// fn process_instruction(
///     program_id: &Pubkey,
///     accounts: &[AccountInfo],
///     instruction_data: &[u8],
/// ) -> ProgramResult {
///     let instr = CreateAccountInstruction::deserialize(&mut &instruction_data[..])?;
///
///     let account_info_iter = &mut accounts.iter();
///
///     let payer = next_account_info(account_info_iter)?;
///     let new_account_pda = next_account_info(account_info_iter)?;
///     let system_account = next_account_info(account_info_iter)?;
///
///     assert!(payer.is_signer);
///     assert!(payer.is_writable);
///     // Note that `new_account_pda` is not a signer yet.
///     // This program will sign for it via `invoke_signed`.
///     assert!(!new_account_pda.is_signer);
///     assert!(new_account_pda.is_writable);
///     assert!(system_program::check_id(system_account.key));
///
///     let new_account_seed = &instr.new_account_seed;
///     let new_account_bump_seed = instr.new_account_bump_seed;
///
///     let rent = Rent::get()?
///         .minimum_balance(instr.space.try_into().expect("overflow"));
///
///     invoke_signed(
///         &system_instruction::transfer(
///             payer.key,
///             new_account_pda.key,
///             rent,
///         ),
///         &[payer.clone(), new_account_pda.clone()],
///         &[&[payer.key.as_ref(), new_account_seed, &[new_account_bump_seed]]],
///     )?;
///
///     invoke_signed(
///         &system_instruction::allocate(
///             new_account_pda.key,
///             instr.space,
///         ),
///         &[new_account_pda.clone()],
///         &[&[payer.key.as_ref(), new_account_seed, &[new_account_bump_seed]]],
///     )?;
///
///     invoke_signed(
///         &system_instruction::assign(
///             new_account_pda.key,
///             &program_id,
///         ),
///         &[new_account_pda.clone()],
///         &[&[payer.key.as_ref(), new_account_seed, &[new_account_bump_seed]]],
///     )?;
///
///     Ok(())
/// }
///
/// # Ok::<(), anyhow::Error>(())
/// ```
pub fn allocate(pubkey: &Pubkey, space: u64) -> Instruction {
    let account_metas = vec![AccountMeta::new(*pubkey, true)];
    Instruction::new_with_bincode(
        system_program::id(),
        &SystemInstruction::Allocate { space },
        account_metas,
    )
}

pub fn allocate_with_seed(
    address: &Pubkey, // must match create_with_seed(base, seed, owner)
    base: &Pubkey,
    seed: &str,
    space: u64,
    owner: &Pubkey,
) -> Instruction {
    let account_metas = vec![
        AccountMeta::new(*address, false),
        AccountMeta::new_readonly(*base, true),
    ];
    Instruction::new_with_bincode(
        system_program::id(),
        &SystemInstruction::AllocateWithSeed {
            base: *base,
            seed: seed.to_string(),
            space,
            owner: *owner,
        },
        account_metas,
    )
}

/// Transfer lamports from an account owned by the system program to multiple accounts.
///
/// This function produces a vector of [`Instruction`]s which must be submitted
/// in a [`Transaction`] or [invoked] to take effect, containing serialized
/// [`SystemInstruction::Transfer`]s.
///
/// [`Transaction`]: https://docs.rs/solana-sdk/latest/solana_sdk/transaction/struct.Transaction.html
/// [invoked]: crate::program::invoke
///
/// # Required signers
///
/// The `from_pubkey` signer must sign the transaction.
///
/// # Examples
///
/// ## Example: client-side RPC
///
/// This example performs multiple transfers in a single transaction.
///
/// ```
/// # use solana_program::example_mocks::{solana_sdk, solana_rpc_client};
/// use solana_rpc_client::rpc_client::RpcClient;
/// use solana_sdk::{
///     pubkey::Pubkey,
///     signature::{Keypair, Signer},
///     system_instruction,
///     transaction::Transaction,
/// };
/// use anyhow::Result;
///
/// fn transfer_lamports_to_many(
///     client: &RpcClient,
///     from: &Keypair,
///     to_and_amount: &[(Pubkey, u64)],
/// ) -> Result<()> {
///     let instrs = system_instruction::transfer_many(&from.pubkey(), to_and_amount);
///
///     let blockhash = client.get_latest_blockhash()?;
///     let tx = Transaction::new_signed_with_payer(
///         &instrs,
///         Some(&from.pubkey()),
///         &[from],
///         blockhash,
///     );
///
///     let _sig = client.send_and_confirm_transaction(&tx)?;
///
///     Ok(())
/// }
/// # let from = Keypair::new();
/// # let to_and_amount = vec![
/// #     (Pubkey::new_unique(), 1_000),
/// #     (Pubkey::new_unique(), 2_000),
/// #     (Pubkey::new_unique(), 3_000),
/// # ];
/// # let client = RpcClient::new(String::new());
/// # transfer_lamports_to_many(&client, &from, &to_and_amount);
/// #
/// # Ok::<(), anyhow::Error>(())
/// ```
///
/// ## Example: on-chain program
///
/// This example makes multiple transfers out of a "bank" account,
/// a [program derived address][pda] owned by the calling program.
/// This example submits the instructions from an on-chain Solana program. The
/// created account is a [program derived address][pda], and it is assigned to
/// the running program. The `payer` and `new_account_pda` are signers, with
/// `new_account_pda` being signed for virtually by the program itself via
/// [`invoke_signed`], `payer` being signed for by the client that submitted the
/// transaction.
///
/// [pda]: Pubkey::find_program_address
/// [`invoke_signed`]: crate::program::invoke_signed
///
/// ```
/// # use borsh::{BorshDeserialize, BorshSerialize};
/// use solana_program::{
///     account_info::{next_account_info, next_account_infos, AccountInfo},
///     entrypoint,
///     entrypoint::ProgramResult,
///     msg,
///     program::invoke_signed,
///     pubkey::Pubkey,
///     system_instruction,
///     system_program,
/// };
///
/// /// # Accounts
/// ///
/// /// - 0: bank_pda - writable
/// /// - 1: system_program - executable
/// /// - *: to - writable
/// #[derive(BorshSerialize, BorshDeserialize, Debug)]
/// pub struct TransferLamportsToManyInstruction {
///     pub bank_pda_bump_seed: u8,
///     pub amount_list: Vec<u64>,
/// }
///
/// entrypoint!(process_instruction);
///
/// fn process_instruction(
///     program_id: &Pubkey,
///     accounts: &[AccountInfo],
///     instruction_data: &[u8],
/// ) -> ProgramResult {
///     let instr = TransferLamportsToManyInstruction::deserialize(&mut &instruction_data[..])?;
///
///     let account_info_iter = &mut accounts.iter();
///
///     let bank_pda = next_account_info(account_info_iter)?;
///     let bank_pda_bump_seed = instr.bank_pda_bump_seed;
///     let system_account = next_account_info(account_info_iter)?;
///
///     assert!(system_program::check_id(system_account.key));
///
///     let to_accounts = next_account_infos(account_info_iter, account_info_iter.len())?;
///
///     for to_account in to_accounts {
///          assert!(to_account.is_writable);
///          // ... do other verification ...
///     }
///
///     let to_and_amount = to_accounts
///         .iter()
///         .zip(instr.amount_list.iter())
///         .map(|(to, amount)| (*to.key, *amount))
///         .collect::<Vec<(Pubkey, u64)>>();
///
///     let instrs = system_instruction::transfer_many(bank_pda.key, to_and_amount.as_ref());
///
///     for instr in instrs {
///         invoke_signed(&instr, accounts, &[&[b"bank", &[bank_pda_bump_seed]]])?;
///     }
///
///     Ok(())
/// }
///
/// # Ok::<(), anyhow::Error>(())
/// ```
pub fn transfer_many(from_pubkey: &Pubkey, to_lamports: &[(Pubkey, u64)]) -> Vec<Instruction> {
    to_lamports
        .iter()
        .map(|(to_pubkey, lamports)| transfer(from_pubkey, to_pubkey, *lamports))
        .collect()
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
            nonce::State::size() as u64,
            &system_program::id(),
        ),
        Instruction::new_with_bincode(
            system_program::id(),
            &SystemInstruction::InitializeNonceAccount(*authority),
            vec![
                AccountMeta::new(*nonce_pubkey, false),
                #[allow(deprecated)]
                AccountMeta::new_readonly(recent_blockhashes::id(), false),
                AccountMeta::new_readonly(rent::id(), false),
            ],
        ),
    ]
}

/// Create an account containing a durable transaction nonce.
///
/// This function produces a vector of [`Instruction`]s which must be submitted
/// in a [`Transaction`] or [invoked] to take effect, containing a serialized
/// [`SystemInstruction::CreateAccount`] and
/// [`SystemInstruction::InitializeNonceAccount`].
///
/// [`Transaction`]: https://docs.rs/solana-sdk/latest/solana_sdk/transaction/struct.Transaction.html
/// [invoked]: crate::program::invoke
///
/// A [durable transaction nonce][dtn] is a special account that enables
/// execution of transactions that have been signed in the past.
///
/// Standard Solana transactions include a [recent blockhash][rbh] (sometimes
/// referred to as a _[nonce]_). During execution the Solana runtime verifies
/// the recent blockhash is approximately less than two minutes old, and that in
/// those two minutes no other identical transaction with the same blockhash has
/// been executed. These checks prevent accidental replay of transactions.
/// Consequently, it is not possible to sign a transaction, wait more than two
/// minutes, then successfully execute that transaction.
///
/// [dtn]: https://docs.solana.com/implemented-proposals/durable-tx-nonces
/// [rbh]: crate::message::Message::recent_blockhash
/// [nonce]: https://en.wikipedia.org/wiki/Cryptographic_nonce
///
/// Durable transaction nonces are an alternative to the standard recent
/// blockhash nonce. They are stored in accounts on chain, and every time they
/// are used their value is changed to a new value for their next use. The
/// runtime verifies that each durable nonce value is only used once, and there
/// are no restrictions on how "old" the nonce is. Because they are stored on
/// chain and require additional instructions to use, transacting with durable
/// transaction nonces is more expensive than with standard transactions.
///
/// The value of the durable nonce is itself a blockhash and is accessible via
/// the [`blockhash`] field of [`nonce::state::Data`], which is deserialized
/// from the nonce account data.
///
/// [`blockhash`]: crate::nonce::state::Data::blockhash
/// [`nonce::state::Data`]: crate::nonce::state::Data
///
/// The basic durable transaction nonce lifecycle is
///
/// 1) Create the nonce account with the `create_nonce_account` instruction.
/// 2) Submit specially-formed transactions that include the
///    [`advance_nonce_account`] instruction.
/// 3) Destroy the nonce account by withdrawing its lamports with the
///    [`withdraw_nonce_account`] instruction.
///
/// Nonce accounts have an associated _authority_ account, which is stored in
/// their account data, and can be changed with the [`authorize_nonce_account`]
/// instruction. The authority must sign transactions that include the
/// `advance_nonce_account`, `authorize_nonce_account` and
/// `withdraw_nonce_account` instructions.
///
/// Nonce accounts are owned by the system program.
///
/// This constructor creates a [`SystemInstruction::CreateAccount`] instruction
/// and a [`SystemInstruction::InitializeNonceAccount`] instruction.
///
/// # Required signers
///
/// The `from_pubkey` and `nonce_pubkey` signers must sign the transaction.
///
/// # Examples
///
/// Create a nonce account from an off-chain client:
///
/// ```
/// # use solana_program::example_mocks::solana_sdk;
/// # use solana_program::example_mocks::solana_rpc_client;
/// use solana_rpc_client::rpc_client::RpcClient;
/// use solana_sdk::{
/// #   pubkey::Pubkey,
///     signature::{Keypair, Signer},
///     system_instruction,
///     transaction::Transaction,
///     nonce::State,
/// };
/// use anyhow::Result;
///
/// fn submit_create_nonce_account_tx(
///     client: &RpcClient,
///     payer: &Keypair,
/// ) -> Result<()> {
///
///     let nonce_account = Keypair::new();
///
///     let nonce_rent = client.get_minimum_balance_for_rent_exemption(State::size())?;
///     let instr = system_instruction::create_nonce_account(
///         &payer.pubkey(),
///         &nonce_account.pubkey(),
///         &payer.pubkey(), // Make the fee payer the nonce account authority
///         nonce_rent,
///     );
///
///     let mut tx = Transaction::new_with_payer(&instr, Some(&payer.pubkey()));
///
///     let blockhash = client.get_latest_blockhash()?;
///     tx.try_sign(&[&nonce_account, payer], blockhash)?;
///
///     client.send_and_confirm_transaction(&tx)?;
///
///     Ok(())
/// }
/// #
/// # let client = RpcClient::new(String::new());
/// # let payer = Keypair::new();
/// # submit_create_nonce_account_tx(&client, &payer)?;
/// #
/// # Ok::<(), anyhow::Error>(())
/// ```
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
            nonce::State::size() as u64,
            &system_program::id(),
        ),
        Instruction::new_with_bincode(
            system_program::id(),
            &SystemInstruction::InitializeNonceAccount(*authority),
            vec![
                AccountMeta::new(*nonce_pubkey, false),
                #[allow(deprecated)]
                AccountMeta::new_readonly(recent_blockhashes::id(), false),
                AccountMeta::new_readonly(rent::id(), false),
            ],
        ),
    ]
}

/// Advance the value of a durable transaction nonce.
///
/// This function produces an [`Instruction`] which must be submitted in a
/// [`Transaction`] or [invoked] to take effect, containing a serialized
/// [`SystemInstruction::AdvanceNonceAccount`].
///
/// [`Transaction`]: https://docs.rs/solana-sdk/latest/solana_sdk/transaction/struct.Transaction.html
/// [invoked]: crate::program::invoke
///
/// Every transaction that relies on a durable transaction nonce must contain a
/// [`SystemInstruction::AdvanceNonceAccount`] instruction as the first
/// instruction in the [`Message`], as created by this function. When included
/// in the first position, the Solana runtime recognizes the transaction as one
/// that relies on a durable transaction nonce and processes it accordingly. The
/// [`Message::new_with_nonce`] function can be used to construct a `Message` in
/// the correct format without calling `advance_nonce_account` directly.
///
/// When constructing a transaction that includes an `AdvanceNonceInstruction`
/// the [`recent_blockhash`] must be treated differently &mdash; instead of
/// setting it to a recent blockhash, the value of the nonce must be retreived
/// and deserialized from the nonce account, and that value specified as the
/// "recent blockhash". A nonce account can be deserialized with the
/// [`solana_rpc_client_nonce_utils::data_from_account`][dfa] function.
///
/// For further description of durable transaction nonces see
/// [`create_nonce_account`].
///
/// [`Message`]: crate::message::Message
/// [`Message::new_with_nonce`]: crate::message::Message::new_with_nonce
/// [`recent_blockhash`]: crate::message::Message::recent_blockhash
/// [dfa]: https://docs.rs/solana-rpc-client-nonce-utils/latest/solana_rpc_client_nonce_utils/fn.data_from_account.html
///
/// # Required signers
///
/// The `authorized_pubkey` signer must sign the transaction.
///
/// # Examples
///
/// Create and sign a transaction with a durable nonce:
///
/// ```
/// # use solana_program::example_mocks::solana_sdk;
/// # use solana_program::example_mocks::solana_rpc_client;
/// # use solana_program::example_mocks::solana_rpc_client_nonce_utils;
/// use solana_rpc_client::rpc_client::RpcClient;
/// use solana_sdk::{
///     message::Message,
///     pubkey::Pubkey,
///     signature::{Keypair, Signer},
///     system_instruction,
///     transaction::Transaction,
/// };
/// # use solana_sdk::account::Account;
/// use std::path::Path;
/// use anyhow::Result;
/// # use anyhow::anyhow;
///
/// fn create_transfer_tx_with_nonce(
///     client: &RpcClient,
///     nonce_account_pubkey: &Pubkey,
///     payer: &Keypair,
///     receiver: &Pubkey,
///     amount: u64,
///     tx_path: &Path,
/// ) -> Result<()> {
///
///     let instr_transfer = system_instruction::transfer(
///         &payer.pubkey(),
///         receiver,
///         amount,
///     );
///
///     // In this example, `payer` is `nonce_account_pubkey`'s authority
///     let instr_advance_nonce_account = system_instruction::advance_nonce_account(
///         nonce_account_pubkey,
///         &payer.pubkey(),
///     );
///
///     // The `advance_nonce_account` instruction must be the first issued in
///     // the transaction.
///     let message = Message::new(
///         &[
///             instr_advance_nonce_account,
///             instr_transfer
///         ],
///         Some(&payer.pubkey()),
///     );
///
///     let mut tx = Transaction::new_unsigned(message);
///
///     // Sign the tx with nonce_account's `blockhash` instead of the
///     // network's latest blockhash.
///     # client.set_get_account_response(*nonce_account_pubkey, Account {
///     #   lamports: 1,
///     #   data: vec![0],
///     #   owner: solana_sdk::system_program::ID,
///     #   executable: false,
///     #   rent_epoch: 1,
///     # });
///     let nonce_account = client.get_account(nonce_account_pubkey)?;
///     let nonce_data = solana_rpc_client_nonce_utils::data_from_account(&nonce_account)?;
///     let blockhash = nonce_data.blockhash();
///
///     tx.try_sign(&[payer], blockhash)?;
///
///     // Save the signed transaction locally for later submission.
///     save_tx_to_file(&tx_path, &tx)?;
///
///     Ok(())
/// }
/// #
/// # fn save_tx_to_file(path: &Path, tx: &Transaction) -> Result<()> {
/// #     Ok(())
/// # }
/// #
/// # let client = RpcClient::new(String::new());
/// # let nonce_account_pubkey = Pubkey::new_unique();
/// # let payer = Keypair::new();
/// # let receiver = Pubkey::new_unique();
/// # create_transfer_tx_with_nonce(&client, &nonce_account_pubkey, &payer, &receiver, 1024, Path::new("new_tx"))?;
/// #
/// # Ok::<(), anyhow::Error>(())
/// ```
pub fn advance_nonce_account(nonce_pubkey: &Pubkey, authorized_pubkey: &Pubkey) -> Instruction {
    let account_metas = vec![
        AccountMeta::new(*nonce_pubkey, false),
        #[allow(deprecated)]
        AccountMeta::new_readonly(recent_blockhashes::id(), false),
        AccountMeta::new_readonly(*authorized_pubkey, true),
    ];
    Instruction::new_with_bincode(
        system_program::id(),
        &SystemInstruction::AdvanceNonceAccount,
        account_metas,
    )
}

/// Withdraw lamports from a durable transaction nonce account.
///
/// This function produces an [`Instruction`] which must be submitted in a
/// [`Transaction`] or [invoked] to take effect, containing a serialized
/// [`SystemInstruction::WithdrawNonceAccount`].
///
/// [`Transaction`]: https://docs.rs/solana-sdk/latest/solana_sdk/transaction/struct.Transaction.html
/// [invoked]: crate::program::invoke
///
/// Withdrawing the entire balance of a nonce account will cause the runtime to
/// destroy it upon successful completion of the transaction.
///
/// Otherwise, nonce accounts must maintain a balance greater than or equal to
/// the minimum required for [rent exemption]. If the result of this instruction
/// would leave the nonce account with a balance less than required for rent
/// exemption, but also greater than zero, then the transaction will fail.
///
/// [rent exemption]: https://docs.solana.com/developing/programming-model/accounts#rent-exemption
///
/// This constructor creates a [`SystemInstruction::WithdrawNonceAccount`]
/// instruction.
///
/// # Required signers
///
/// The `authorized_pubkey` signer must sign the transaction.
///
/// # Examples
///
/// ```
/// # use solana_program::example_mocks::solana_sdk;
/// # use solana_program::example_mocks::solana_rpc_client;
/// use solana_rpc_client::rpc_client::RpcClient;
/// use solana_sdk::{
///     pubkey::Pubkey,
///     signature::{Keypair, Signer},
///     system_instruction,
///     transaction::Transaction,
/// };
/// use anyhow::Result;
///
/// fn submit_withdraw_nonce_account_tx(
///     client: &RpcClient,
///     nonce_account_pubkey: &Pubkey,
///     authorized_account: &Keypair,
/// ) -> Result<()> {
///
///     let nonce_balance = client.get_balance(nonce_account_pubkey)?;
///
///     let instr = system_instruction::withdraw_nonce_account(
///         &nonce_account_pubkey,
///         &authorized_account.pubkey(),
///         &authorized_account.pubkey(),
///         nonce_balance,
///     );
///
///     let mut tx = Transaction::new_with_payer(&[instr], Some(&authorized_account.pubkey()));
///
///     let blockhash = client.get_latest_blockhash()?;
///     tx.try_sign(&[authorized_account], blockhash)?;
///
///     client.send_and_confirm_transaction(&tx)?;
///
///     Ok(())
/// }
/// #
/// # let client = RpcClient::new(String::new());
/// # let nonce_account_pubkey = Pubkey::new_unique();
/// # let payer = Keypair::new();
/// # submit_withdraw_nonce_account_tx(&client, &nonce_account_pubkey, &payer)?;
/// #
/// # Ok::<(), anyhow::Error>(())
/// ```
pub fn withdraw_nonce_account(
    nonce_pubkey: &Pubkey,
    authorized_pubkey: &Pubkey,
    to_pubkey: &Pubkey,
    lamports: u64,
) -> Instruction {
    let account_metas = vec![
        AccountMeta::new(*nonce_pubkey, false),
        AccountMeta::new(*to_pubkey, false),
        #[allow(deprecated)]
        AccountMeta::new_readonly(recent_blockhashes::id(), false),
        AccountMeta::new_readonly(rent::id(), false),
        AccountMeta::new_readonly(*authorized_pubkey, true),
    ];
    Instruction::new_with_bincode(
        system_program::id(),
        &SystemInstruction::WithdrawNonceAccount(lamports),
        account_metas,
    )
}

/// Change the authority of a durable transaction nonce account.
///
/// This function produces an [`Instruction`] which must be submitted in a
/// [`Transaction`] or [invoked] to take effect, containing a serialized
/// [`SystemInstruction::AuthorizeNonceAccount`].
///
/// [`Transaction`]: https://docs.rs/solana-sdk/latest/solana_sdk/transaction/struct.Transaction.html
/// [invoked]: crate::program::invoke
///
/// This constructor creates a [`SystemInstruction::AuthorizeNonceAccount`]
/// instruction.
///
/// # Required signers
///
/// The `authorized_pubkey` signer must sign the transaction.
///
/// # Examples
///
/// ```
/// # use solana_program::example_mocks::solana_sdk;
/// # use solana_program::example_mocks::solana_rpc_client;
/// use solana_rpc_client::rpc_client::RpcClient;
/// use solana_sdk::{
///     pubkey::Pubkey,
///     signature::{Keypair, Signer},
///     system_instruction,
///     transaction::Transaction,
/// };
/// use anyhow::Result;
///
/// fn authorize_nonce_account_tx(
///     client: &RpcClient,
///     nonce_account_pubkey: &Pubkey,
///     authorized_account: &Keypair,
///     new_authority_pubkey: &Pubkey,
/// ) -> Result<()> {
///
///     let instr = system_instruction::authorize_nonce_account(
///         &nonce_account_pubkey,
///         &authorized_account.pubkey(),
///         &new_authority_pubkey,
///     );
///
///     let mut tx = Transaction::new_with_payer(&[instr], Some(&authorized_account.pubkey()));
///
///     let blockhash = client.get_latest_blockhash()?;
///     tx.try_sign(&[authorized_account], blockhash)?;
///
///     client.send_and_confirm_transaction(&tx)?;
///
///     Ok(())
/// }
/// #
/// # let client = RpcClient::new(String::new());
/// # let nonce_account_pubkey = Pubkey::new_unique();
/// # let payer = Keypair::new();
/// # let new_authority_pubkey = Pubkey::new_unique();
/// # authorize_nonce_account_tx(&client, &nonce_account_pubkey, &payer, &new_authority_pubkey)?;
/// #
/// # Ok::<(), anyhow::Error>(())
/// ```
pub fn authorize_nonce_account(
    nonce_pubkey: &Pubkey,
    authorized_pubkey: &Pubkey,
    new_authority: &Pubkey,
) -> Instruction {
    let account_metas = vec![
        AccountMeta::new(*nonce_pubkey, false),
        AccountMeta::new_readonly(*authorized_pubkey, true),
    ];
    Instruction::new_with_bincode(
        system_program::id(),
        &SystemInstruction::AuthorizeNonceAccount(*new_authority),
        account_metas,
    )
}

/// One-time idempotent upgrade of legacy nonce versions in order to bump
/// them out of chain blockhash domain.
pub fn upgrade_nonce_account(nonce_pubkey: Pubkey) -> Instruction {
    let account_metas = vec![AccountMeta::new(nonce_pubkey, /*is_signer:*/ false)];
    Instruction::new_with_bincode(
        system_program::id(),
        &SystemInstruction::UpgradeNonceAccount,
        account_metas,
    )
}

#[cfg(test)]
mod tests {
    use {super::*, crate::instruction::Instruction};

    fn get_keys(instruction: &Instruction) -> Vec<Pubkey> {
        instruction.accounts.iter().map(|x| x.pubkey).collect()
    }

    #[test]
    fn test_move_many() {
        let alice_pubkey = Pubkey::new_unique();
        let bob_pubkey = Pubkey::new_unique();
        let carol_pubkey = Pubkey::new_unique();
        let to_lamports = vec![(bob_pubkey, 1), (carol_pubkey, 2)];

        let instructions = transfer_many(&alice_pubkey, &to_lamports);
        assert_eq!(instructions.len(), 2);
        assert_eq!(get_keys(&instructions[0]), vec![alice_pubkey, bob_pubkey]);
        assert_eq!(get_keys(&instructions[1]), vec![alice_pubkey, carol_pubkey]);
    }

    #[test]
    fn test_create_nonce_account() {
        let from_pubkey = Pubkey::new_unique();
        let nonce_pubkey = Pubkey::new_unique();
        let authorized = nonce_pubkey;
        let ixs = create_nonce_account(&from_pubkey, &nonce_pubkey, &authorized, 42);
        assert_eq!(ixs.len(), 2);
        let ix = &ixs[0];
        assert_eq!(ix.program_id, system_program::id());
        let pubkeys: Vec<_> = ix.accounts.iter().map(|am| am.pubkey).collect();
        assert!(pubkeys.contains(&from_pubkey));
        assert!(pubkeys.contains(&nonce_pubkey));
    }
}
