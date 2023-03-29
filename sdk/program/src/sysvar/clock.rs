//! Information about the network’s clock, ticks, slots, etc.
//!
//! The _clock sysvar_ provides access to the [`Clock`] type, which includes the
//! current slot, the current epoch, and the approximate real-world time of the
//! slot.
//!
//! [`Clock`] implements [`Sysvar::get`] and can be loaded efficiently without
//! passing the sysvar account ID to the program.
//!
//! See also the Solana [documentation on the clock sysvar][sdoc].
//!
//! [sdoc]: https://docs.solana.com/developing/runtime-facilities/sysvars#clock
//!
//! # Examples
//!
//! Accessing via on-chain program directly:
//!
//! ```no_run
//! # use solana_program::{
//! #    account_info::{AccountInfo, next_account_info},
//! #    entrypoint::ProgramResult,
//! #    msg,
//! #    pubkey::Pubkey,
//! #    sysvar::clock::{self, Clock},
//! #    sysvar::Sysvar,
//! # };
//! # use solana_program::program_error::ProgramError;
//! #
//! fn process_instruction(
//!     program_id: &Pubkey,
//!     accounts: &[AccountInfo],
//!     instruction_data: &[u8],
//! ) -> ProgramResult {
//!
//!     let clock = Clock::get()?;
//!     msg!("clock: {:#?}", clock);
//!
//!     Ok(())
//! }
//! #
//! # use solana_program::sysvar::SysvarId;
//! # let p = Clock::id();
//! # let l = &mut 1169280;
//! # let d = &mut vec![240, 153, 233, 7, 0, 0, 0, 0, 11, 115, 118, 98, 0, 0, 0, 0, 51, 1, 0, 0, 0, 0, 0, 0, 52, 1, 0, 0, 0, 0, 0, 0, 121, 50, 119, 98, 0, 0, 0, 0];
//! # let a = AccountInfo::new(&p, false, false, l, d, &p, false, 0);
//! # let accounts = &[a.clone(), a];
//! # process_instruction(
//! #     &Pubkey::new_unique(),
//! #     accounts,
//! #     &[],
//! # )?;
//! # Ok::<(), ProgramError>(())
//! ```
//!
//! Accessing via on-chain program's account parameters:
//!
//! ```
//! # use solana_program::{
//! #    account_info::{AccountInfo, next_account_info},
//! #    entrypoint::ProgramResult,
//! #    msg,
//! #    pubkey::Pubkey,
//! #    sysvar::clock::{self, Clock},
//! #    sysvar::Sysvar,
//! # };
//! # use solana_program::program_error::ProgramError;
//! #
//! fn process_instruction(
//!     program_id: &Pubkey,
//!     accounts: &[AccountInfo],
//!     instruction_data: &[u8],
//! ) -> ProgramResult {
//!     let account_info_iter = &mut accounts.iter();
//!     let clock_account_info = next_account_info(account_info_iter)?;
//!
//!     assert!(clock::check_id(clock_account_info.key));
//!
//!     let clock = Clock::from_account_info(clock_account_info)?;
//!     msg!("clock: {:#?}", clock);
//!
//!     Ok(())
//! }
//! #
//! # use solana_program::sysvar::SysvarId;
//! # let p = Clock::id();
//! # let l = &mut 1169280;
//! # let d = &mut vec![240, 153, 233, 7, 0, 0, 0, 0, 11, 115, 118, 98, 0, 0, 0, 0, 51, 1, 0, 0, 0, 0, 0, 0, 52, 1, 0, 0, 0, 0, 0, 0, 121, 50, 119, 98, 0, 0, 0, 0];
//! # let a = AccountInfo::new(&p, false, false, l, d, &p, false, 0);
//! # let accounts = &[a.clone(), a];
//! # process_instruction(
//! #     &Pubkey::new_unique(),
//! #     accounts,
//! #     &[],
//! # )?;
//! # Ok::<(), ProgramError>(())
//! ```
//!
//! Accessing via the RPC client:
//!
//! ```
//! # use solana_program::example_mocks::solana_sdk;
//! # use solana_program::example_mocks::solana_rpc_client;
//! # use solana_sdk::account::Account;
//! # use solana_rpc_client::rpc_client::RpcClient;
//! # use solana_sdk::sysvar::clock::{self, Clock};
//! # use anyhow::Result;
//! #
//! fn print_sysvar_clock(client: &RpcClient) -> Result<()> {
//! #   client.set_get_account_response(clock::ID, Account {
//! #       lamports: 1169280,
//! #       data: vec![240, 153, 233, 7, 0, 0, 0, 0, 11, 115, 118, 98, 0, 0, 0, 0, 51, 1, 0, 0, 0, 0, 0, 0, 52, 1, 0, 0, 0, 0, 0, 0, 121, 50, 119, 98, 0, 0, 0, 0],
//! #       owner: solana_sdk::system_program::ID,
//! #       executable: false,
//! #       rent_epoch: 307,
//! #   });
//! #
//!     let clock = client.get_account(&clock::ID)?;
//!     let data: Clock = bincode::deserialize(&clock.data)?;
//!
//!     Ok(())
//! }
//! #
//! # let client = RpcClient::new(String::new());
//! # print_sysvar_clock(&client)?;
//! #
//! # Ok::<(), anyhow::Error>(())
//! ```

pub use crate::clock::Clock;
use crate::{impl_sysvar_get, program_error::ProgramError, sysvar::Sysvar};

crate::declare_sysvar_id!("SysvarC1ock11111111111111111111111111111111", Clock);

impl Sysvar for Clock {
    impl_sysvar_get!(sol_get_clock_sysvar);
}
