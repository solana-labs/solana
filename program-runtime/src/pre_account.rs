use {
    crate::timings::ExecuteDetailsTimings,
    solana_sdk::{
        account::{AccountSharedData, ReadableAccount, WritableAccount},
        instruction::InstructionError,
        pubkey::Pubkey,
        rent::Rent,
        system_instruction::MAX_PERMITTED_DATA_LENGTH,
    },
    std::fmt::Debug,
};

// The relevant state of an account before an Instruction executes, used
// to verify account integrity after the Instruction completes
#[derive(Clone, Debug, Default)]
pub struct PreAccount {
    key: Pubkey,
    account: AccountSharedData,
    changed: bool,
}
impl PreAccount {
    pub fn new(key: &Pubkey, account: AccountSharedData) -> Self {
        Self {
            key: *key,
            account,
            changed: false,
        }
    }

    pub fn verify(
        &self,
        program_id: &Pubkey,
        is_writable: bool,
        rent: &Rent,
        post: &AccountSharedData,
        timings: &mut ExecuteDetailsTimings,
        outermost_call: bool,
    ) -> Result<(), InstructionError> {
        let pre = &self.account;

        // Only the owner of the account may change owner and
        //   only if the account is writable and
        //   only if the account is not executable and
        //   only if the data is zero-initialized or empty
        let owner_changed = pre.owner() != post.owner();
        if owner_changed
            && (!is_writable // line coverage used to get branch coverage
                || pre.executable()
                || program_id != pre.owner()
            || !Self::is_zeroed(post.data()))
        {
            return Err(InstructionError::ModifiedProgramId);
        }

        // An account not assigned to the program cannot have its balance decrease.
        if program_id != pre.owner() // line coverage used to get branch coverage
         && pre.lamports() > post.lamports()
        {
            return Err(InstructionError::ExternalAccountLamportSpend);
        }

        // The balance of read-only and executable accounts may not change
        let lamports_changed = pre.lamports() != post.lamports();
        if lamports_changed {
            if !is_writable {
                return Err(InstructionError::ReadonlyLamportChange);
            }
            if pre.executable() {
                return Err(InstructionError::ExecutableLamportChange);
            }
        }

        // Account data size cannot exceed a maxumum length
        if post.data().len() > MAX_PERMITTED_DATA_LENGTH as usize {
            return Err(InstructionError::InvalidRealloc);
        }

        // The owner of the account can change the size of the data
        let data_len_changed = pre.data().len() != post.data().len();
        if data_len_changed && program_id != pre.owner() {
            return Err(InstructionError::AccountDataSizeChanged);
        }

        // Only the owner may change account data
        //   and if the account is writable
        //   and if the account is not executable
        if !(program_id == pre.owner()
            && is_writable  // line coverage used to get branch coverage
            && !pre.executable())
            && pre.data() != post.data()
        {
            if pre.executable() {
                return Err(InstructionError::ExecutableDataModified);
            } else if is_writable {
                return Err(InstructionError::ExternalAccountDataModified);
            } else {
                return Err(InstructionError::ReadonlyDataModified);
            }
        }

        // executable is one-way (false->true) and only the account owner may set it.
        let executable_changed = pre.executable() != post.executable();
        if executable_changed {
            if !rent.is_exempt(post.lamports(), post.data().len()) {
                return Err(InstructionError::ExecutableAccountNotRentExempt);
            }
            if !is_writable // line coverage used to get branch coverage
                || pre.executable()
                || program_id != post.owner()
            {
                return Err(InstructionError::ExecutableModified);
            }
        }

        // No one modifies rent_epoch (yet).
        let rent_epoch_changed = pre.rent_epoch() != post.rent_epoch();
        if rent_epoch_changed {
            return Err(InstructionError::RentEpochModified);
        }

        if outermost_call {
            timings.total_account_count = timings.total_account_count.saturating_add(1);
            if owner_changed
                || lamports_changed
                || data_len_changed
                || executable_changed
                || rent_epoch_changed
                || self.changed
            {
                timings.changed_account_count = timings.changed_account_count.saturating_add(1);
            }
        }

        Ok(())
    }

    pub fn update(&mut self, account: AccountSharedData) {
        let rent_epoch = self.account.rent_epoch();
        self.account = account;
        self.account.set_rent_epoch(rent_epoch);

        self.changed = true;
    }

    pub fn key(&self) -> &Pubkey {
        &self.key
    }

    pub fn data(&self) -> &[u8] {
        self.account.data()
    }

    pub fn lamports(&self) -> u64 {
        self.account.lamports()
    }

    pub fn executable(&self) -> bool {
        self.account.executable()
    }

    pub fn is_zeroed(buf: &[u8]) -> bool {
        const ZEROS_LEN: usize = 1024;
        static ZEROS: [u8; ZEROS_LEN] = [0; ZEROS_LEN];
        let mut chunks = buf.chunks_exact(ZEROS_LEN);

        #[allow(clippy::indexing_slicing)]
        {
            chunks.all(|chunk| chunk == &ZEROS[..])
                && chunks.remainder() == &ZEROS[..chunks.remainder().len()]
        }
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        solana_sdk::{account::Account, instruction::InstructionError, system_program},
    };

    #[test]
    fn test_is_zeroed() {
        const ZEROS_LEN: usize = 1024;
        let mut buf = [0; ZEROS_LEN];
        assert!(PreAccount::is_zeroed(&buf));
        buf[0] = 1;
        assert!(!PreAccount::is_zeroed(&buf));

        let mut buf = [0; ZEROS_LEN - 1];
        assert!(PreAccount::is_zeroed(&buf));
        buf[0] = 1;
        assert!(!PreAccount::is_zeroed(&buf));

        let mut buf = [0; ZEROS_LEN + 1];
        assert!(PreAccount::is_zeroed(&buf));
        buf[0] = 1;
        assert!(!PreAccount::is_zeroed(&buf));

        let buf = vec![];
        assert!(PreAccount::is_zeroed(&buf));
    }

    struct Change {
        program_id: Pubkey,
        is_writable: bool,
        rent: Rent,
        pre: PreAccount,
        post: AccountSharedData,
    }
    impl Change {
        pub fn new(owner: &Pubkey, program_id: &Pubkey) -> Self {
            Self {
                program_id: *program_id,
                rent: Rent::default(),
                is_writable: true,
                pre: PreAccount::new(
                    &solana_sdk::pubkey::new_rand(),
                    AccountSharedData::from(Account {
                        owner: *owner,
                        lamports: std::u64::MAX,
                        ..Account::default()
                    }),
                ),
                post: AccountSharedData::from(Account {
                    owner: *owner,
                    lamports: std::u64::MAX,
                    ..Account::default()
                }),
            }
        }
        pub fn read_only(mut self) -> Self {
            self.is_writable = false;
            self
        }
        pub fn executable(mut self, pre: bool, post: bool) -> Self {
            self.pre.account.set_executable(pre);
            self.post.set_executable(post);
            self
        }
        pub fn lamports(mut self, pre: u64, post: u64) -> Self {
            self.pre.account.set_lamports(pre);
            self.post.set_lamports(post);
            self
        }
        pub fn owner(mut self, post: &Pubkey) -> Self {
            self.post.set_owner(*post);
            self
        }
        pub fn data(mut self, pre: Vec<u8>, post: Vec<u8>) -> Self {
            self.pre.account.set_data(pre);
            self.post.set_data(post);
            self
        }
        pub fn rent_epoch(mut self, pre: u64, post: u64) -> Self {
            self.pre.account.set_rent_epoch(pre);
            self.post.set_rent_epoch(post);
            self
        }
        pub fn verify(&self) -> Result<(), InstructionError> {
            self.pre.verify(
                &self.program_id,
                self.is_writable,
                &self.rent,
                &self.post,
                &mut ExecuteDetailsTimings::default(),
                false,
            )
        }
    }

    #[test]
    fn test_verify_account_changes_owner() {
        let system_program_id = system_program::id();
        let alice_program_id = solana_sdk::pubkey::new_rand();
        let mallory_program_id = solana_sdk::pubkey::new_rand();

        assert_eq!(
            Change::new(&system_program_id, &system_program_id)
                .owner(&alice_program_id)
                .verify(),
            Ok(()),
            "system program should be able to change the account owner"
        );
        assert_eq!(
            Change::new(&system_program_id, &system_program_id)
                .owner(&alice_program_id)
                .read_only()
                .verify(),
            Err(InstructionError::ModifiedProgramId),
            "system program should not be able to change the account owner of a read-only account"
        );
        assert_eq!(
            Change::new(&mallory_program_id, &system_program_id)
                .owner(&alice_program_id)
                .verify(),
            Err(InstructionError::ModifiedProgramId),
            "system program should not be able to change the account owner of a non-system account"
        );
        assert_eq!(
            Change::new(&mallory_program_id, &mallory_program_id)
                .owner(&alice_program_id)
                .verify(),
            Ok(()),
            "mallory should be able to change the account owner, if she leaves clear data"
        );
        assert_eq!(
            Change::new(&mallory_program_id, &mallory_program_id)
                .owner(&alice_program_id)
                .data(vec![42], vec![0])
                .verify(),
            Ok(()),
            "mallory should be able to change the account owner, if she leaves clear data"
        );
        assert_eq!(
            Change::new(&mallory_program_id, &mallory_program_id)
                .owner(&alice_program_id)
                .executable(true, true)
                .data(vec![42], vec![0])
                .verify(),
            Err(InstructionError::ModifiedProgramId),
            "mallory should not be able to change the account owner, if the account executable"
        );
        assert_eq!(
            Change::new(&mallory_program_id, &mallory_program_id)
                .owner(&alice_program_id)
                .data(vec![42], vec![42])
                .verify(),
            Err(InstructionError::ModifiedProgramId),
            "mallory should not be able to inject data into the alice program"
        );
    }

    #[test]
    fn test_verify_account_changes_executable() {
        let owner = solana_sdk::pubkey::new_rand();
        let mallory_program_id = solana_sdk::pubkey::new_rand();
        let system_program_id = system_program::id();

        assert_eq!(
            Change::new(&owner, &system_program_id)
                .executable(false, true)
                .verify(),
            Err(InstructionError::ExecutableModified),
            "system program can't change executable if system doesn't own the account"
        );
        assert_eq!(
            Change::new(&owner, &system_program_id)
                .executable(true, true)
                .data(vec![1], vec![2])
                .verify(),
            Err(InstructionError::ExecutableDataModified),
            "system program can't change executable data if system doesn't own the account"
        );
        assert_eq!(
            Change::new(&owner, &owner).executable(false, true).verify(),
            Ok(()),
            "owner should be able to change executable"
        );
        assert_eq!(
            Change::new(&owner, &owner)
                .executable(false, true)
                .read_only()
                .verify(),
            Err(InstructionError::ExecutableModified),
            "owner can't modify executable of read-only accounts"
        );
        assert_eq!(
            Change::new(&owner, &owner).executable(true, false).verify(),
            Err(InstructionError::ExecutableModified),
            "owner program can't reverse executable"
        );
        assert_eq!(
            Change::new(&owner, &mallory_program_id)
                .executable(false, true)
                .verify(),
            Err(InstructionError::ExecutableModified),
            "malicious Mallory should not be able to change the account executable"
        );
        assert_eq!(
            Change::new(&owner, &owner)
                .executable(false, true)
                .data(vec![1], vec![2])
                .verify(),
            Ok(()),
            "account data can change in the same instruction that sets the bit"
        );
        assert_eq!(
            Change::new(&owner, &owner)
                .executable(true, true)
                .data(vec![1], vec![2])
                .verify(),
            Err(InstructionError::ExecutableDataModified),
            "owner should not be able to change an account's data once its marked executable"
        );
        assert_eq!(
            Change::new(&owner, &owner)
                .executable(true, true)
                .lamports(1, 2)
                .verify(),
            Err(InstructionError::ExecutableLamportChange),
            "owner should not be able to add lamports once marked executable"
        );
        assert_eq!(
            Change::new(&owner, &owner)
                .executable(true, true)
                .lamports(1, 2)
                .verify(),
            Err(InstructionError::ExecutableLamportChange),
            "owner should not be able to add lamports once marked executable"
        );
        assert_eq!(
            Change::new(&owner, &owner)
                .executable(true, true)
                .lamports(2, 1)
                .verify(),
            Err(InstructionError::ExecutableLamportChange),
            "owner should not be able to subtract lamports once marked executable"
        );
        let data = vec![1; 100];
        let min_lamports = Rent::default().minimum_balance(data.len());
        assert_eq!(
            Change::new(&owner, &owner)
                .executable(false, true)
                .lamports(0, min_lamports)
                .data(data.clone(), data.clone())
                .verify(),
            Ok(()),
        );
        assert_eq!(
            Change::new(&owner, &owner)
                .executable(false, true)
                .lamports(0, min_lamports - 1)
                .data(data.clone(), data)
                .verify(),
            Err(InstructionError::ExecutableAccountNotRentExempt),
            "owner should not be able to change an account's data once its marked executable"
        );
    }

    #[test]
    fn test_verify_account_changes_data_len() {
        let alice_program_id = solana_sdk::pubkey::new_rand();

        assert_eq!(
            Change::new(&system_program::id(), &system_program::id())
                .data(vec![0], vec![0, 0])
                .verify(),
            Ok(()),
            "system program should be able to change the data len"
        );
        assert_eq!(
            Change::new(&alice_program_id, &system_program::id())
            .data(vec![0], vec![0,0])
            .verify(),
        Err(InstructionError::AccountDataSizeChanged),
        "system program should not be able to change the data length of accounts it does not own"
        );
    }

    #[test]
    fn test_verify_account_changes_data() {
        let alice_program_id = solana_sdk::pubkey::new_rand();
        let mallory_program_id = solana_sdk::pubkey::new_rand();

        assert_eq!(
            Change::new(&alice_program_id, &alice_program_id)
                .data(vec![0], vec![42])
                .verify(),
            Ok(()),
            "alice program should be able to change the data"
        );
        assert_eq!(
            Change::new(&mallory_program_id, &alice_program_id)
                .data(vec![0], vec![42])
                .verify(),
            Err(InstructionError::ExternalAccountDataModified),
            "non-owner mallory should not be able to change the account data"
        );
        assert_eq!(
            Change::new(&alice_program_id, &alice_program_id)
                .data(vec![0], vec![42])
                .read_only()
                .verify(),
            Err(InstructionError::ReadonlyDataModified),
            "alice isn't allowed to touch a CO account"
        );
    }

    #[test]
    fn test_verify_account_changes_rent_epoch() {
        let alice_program_id = solana_sdk::pubkey::new_rand();

        assert_eq!(
            Change::new(&alice_program_id, &system_program::id()).verify(),
            Ok(()),
            "nothing changed!"
        );
        assert_eq!(
            Change::new(&alice_program_id, &system_program::id())
                .rent_epoch(0, 1)
                .verify(),
            Err(InstructionError::RentEpochModified),
            "no one touches rent_epoch"
        );
    }

    #[test]
    fn test_verify_account_changes_deduct_lamports_and_reassign_account() {
        let alice_program_id = solana_sdk::pubkey::new_rand();
        let bob_program_id = solana_sdk::pubkey::new_rand();

        // positive test of this capability
        assert_eq!(
            Change::new(&alice_program_id, &alice_program_id)
            .owner(&bob_program_id)
            .lamports(42, 1)
            .data(vec![42], vec![0])
            .verify(),
        Ok(()),
        "alice should be able to deduct lamports and give the account to bob if the data is zeroed",
    );
    }

    #[test]
    fn test_verify_account_changes_lamports() {
        let alice_program_id = solana_sdk::pubkey::new_rand();

        assert_eq!(
            Change::new(&alice_program_id, &system_program::id())
                .lamports(42, 0)
                .read_only()
                .verify(),
            Err(InstructionError::ExternalAccountLamportSpend),
            "debit should fail, even if system program"
        );
        assert_eq!(
            Change::new(&alice_program_id, &alice_program_id)
                .lamports(42, 0)
                .read_only()
                .verify(),
            Err(InstructionError::ReadonlyLamportChange),
            "debit should fail, even if owning program"
        );
        assert_eq!(
            Change::new(&alice_program_id, &system_program::id())
                .lamports(42, 0)
                .owner(&system_program::id())
                .verify(),
            Err(InstructionError::ModifiedProgramId),
            "system program can't debit the account unless it was the pre.owner"
        );
        assert_eq!(
            Change::new(&system_program::id(), &system_program::id())
                .lamports(42, 0)
                .owner(&alice_program_id)
                .verify(),
            Ok(()),
            "system can spend (and change owner)"
        );
    }

    #[test]
    fn test_verify_account_changes_data_size_changed() {
        let alice_program_id = solana_sdk::pubkey::new_rand();

        assert_eq!(
            Change::new(&alice_program_id, &system_program::id())
                .data(vec![0], vec![0, 0])
                .verify(),
            Err(InstructionError::AccountDataSizeChanged),
            "system program should not be able to change another program's account data size"
        );
        assert_eq!(
            Change::new(&alice_program_id, &solana_sdk::pubkey::new_rand())
                .data(vec![0], vec![0, 0])
                .verify(),
            Err(InstructionError::AccountDataSizeChanged),
            "one program should not be able to change another program's account data size"
        );
        assert_eq!(
            Change::new(&alice_program_id, &alice_program_id)
                .data(vec![0], vec![0, 0])
                .verify(),
            Ok(()),
            "programs can change their own data size"
        );
        assert_eq!(
            Change::new(&system_program::id(), &system_program::id())
                .data(vec![0], vec![0, 0])
                .verify(),
            Ok(()),
            "system program should be able to change account data size"
        );
    }

    #[test]
    fn test_verify_account_changes_owner_executable() {
        let alice_program_id = solana_sdk::pubkey::new_rand();
        let bob_program_id = solana_sdk::pubkey::new_rand();

        assert_eq!(
            Change::new(&alice_program_id, &alice_program_id)
                .owner(&bob_program_id)
                .executable(false, true)
                .verify(),
            Err(InstructionError::ExecutableModified),
            "program should not be able to change owner and executable at the same time"
        );
    }
}
