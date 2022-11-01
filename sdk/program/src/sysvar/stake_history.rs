//! History of stake activations and de-activations.
//!
//! The _stake history sysvar_ provides access to the [`StakeHistory`] type.
//!
//! The [`Sysvar::get`] method always returns
//! [`ProgramError::UnsupportedSysvar`], and in practice the data size of this
//! sysvar is too large to process on chain. One can still use the
//! [`SysvarId::id`], [`SysvarId::check_id`] and [`Sysvar::size_of`] methods in
//! an on-chain program, and it can be accessed off-chain through RPC.
//!
//! [`ProgramError::UnsupportedSysvar`]: crate::program_error::ProgramError::UnsupportedSysvar
//! [`SysvarId::id`]: crate::sysvar::SysvarId::id
//! [`SysvarId::check_id`]: crate::sysvar::SysvarId::check_id
//!
//! # Examples
//!
//! Calling via the RPC client:
//!
//! ```
//! # use solana_program::example_mocks::solana_sdk;
//! # use solana_program::example_mocks::solana_rpc_client;
//! # use solana_sdk::account::Account;
//! # use solana_rpc_client::rpc_client::RpcClient;
//! # use solana_sdk::sysvar::stake_history::{self, StakeHistory};
//! # use anyhow::Result;
//! #
//! fn print_sysvar_stake_history(client: &RpcClient) -> Result<()> {
//! #   client.set_get_account_response(stake_history::ID, Account {
//! #       lamports: 114979200,
//! #       data: vec![0, 0, 0, 0, 0, 0, 0, 0],
//! #       owner: solana_sdk::system_program::ID,
//! #       executable: false,
//! #       rent_epoch: 307,
//! #   });
//! #
//!     let stake_history = client.get_account(&stake_history::ID)?;
//!     let data: StakeHistory = bincode::deserialize(&stake_history.data)?;
//!
//!     Ok(())
//! }
//! #
//! # let client = RpcClient::new(String::new());
//! # print_sysvar_stake_history(&client)?;
//! #
//! # Ok::<(), anyhow::Error>(())
//! ```

pub use crate::stake_history::StakeHistory;
use crate::sysvar::Sysvar;

crate::declare_sysvar_id!("SysvarStakeHistory1111111111111111111111111", StakeHistory);

impl Sysvar for StakeHistory {
    // override
    fn size_of() -> usize {
        // hard-coded so that we don't have to construct an empty
        16392 // golden, update if MAX_ENTRIES changes
    }
}

#[cfg(test)]
mod tests {
    use {super::*, crate::stake_history::*};

    #[test]
    fn test_size_of() {
        let mut stake_history = StakeHistory::default();
        for i in 0..MAX_ENTRIES as u64 {
            stake_history.add(
                i,
                StakeHistoryEntry {
                    activating: i,
                    ..StakeHistoryEntry::default()
                },
            );
        }

        assert_eq!(
            bincode::serialized_size(&stake_history).unwrap() as usize,
            StakeHistory::size_of()
        );
    }

    #[test]
    fn test_create_account() {
        let mut stake_history = StakeHistory::default();
        for i in 0..MAX_ENTRIES as u64 + 1 {
            stake_history.add(
                i,
                StakeHistoryEntry {
                    activating: i,
                    ..StakeHistoryEntry::default()
                },
            );
        }
        assert_eq!(stake_history.len(), MAX_ENTRIES);
        assert_eq!(stake_history.iter().map(|entry| entry.0).min().unwrap(), 1);
        assert_eq!(stake_history.get(0), None);
        assert_eq!(
            stake_history.get(1),
            Some(&StakeHistoryEntry {
                activating: 1,
                ..StakeHistoryEntry::default()
            })
        );
    }
}
