use crate::native_loader::NativeLoader;
use serde::{Deserialize, Serialize};
use solana_sdk::{
    account::{AccountSharedData, ReadableAccount, WritableAccount},
    account_utils::StateMut,
    bpf_loader_upgradeable::{self, UpgradeableLoaderState},
    feature_set::{demote_program_write_locks, do_support_realloc, fix_write_privs},
    ic_msg,
    instruction::{Instruction, InstructionError},
    message::Message,
    process_instruction::{Executor, InvokeContext, ProcessInstructionWithContext},
    pubkey::Pubkey,
    rent::Rent,
    system_instruction::MAX_PERMITTED_DATA_LENGTH,
    system_program,
};
use std::{
    cell::{Ref, RefCell, RefMut},
    collections::HashMap,
    rc::Rc,
    sync::Arc,
};

pub struct Executors {
    pub executors: HashMap<Pubkey, Arc<dyn Executor>>,
    pub is_dirty: bool,
}
impl Default for Executors {
    fn default() -> Self {
        Self {
            executors: HashMap::default(),
            is_dirty: false,
        }
    }
}
impl Executors {
    pub fn insert(&mut self, key: Pubkey, executor: Arc<dyn Executor>) {
        let _ = self.executors.insert(key, executor);
        self.is_dirty = true;
    }
    pub fn get(&self, key: &Pubkey) -> Option<Arc<dyn Executor>> {
        self.executors.get(key).cloned()
    }
}

#[derive(Default, Debug)]
pub struct ProgramTiming {
    pub accumulated_us: u64,
    pub accumulated_units: u64,
    pub count: u32,
}

#[derive(Default, Debug)]
pub struct ExecuteDetailsTimings {
    pub serialize_us: u64,
    pub create_vm_us: u64,
    pub execute_us: u64,
    pub deserialize_us: u64,
    pub changed_account_count: u64,
    pub total_account_count: u64,
    pub total_data_size: usize,
    pub data_size_changed: usize,
    pub per_program_timings: HashMap<Pubkey, ProgramTiming>,
}
impl ExecuteDetailsTimings {
    pub fn accumulate(&mut self, other: &ExecuteDetailsTimings) {
        self.serialize_us += other.serialize_us;
        self.create_vm_us += other.create_vm_us;
        self.execute_us += other.execute_us;
        self.deserialize_us += other.deserialize_us;
        self.changed_account_count += other.changed_account_count;
        self.total_account_count += other.total_account_count;
        self.total_data_size += other.total_data_size;
        self.data_size_changed += other.data_size_changed;
        for (id, other) in &other.per_program_timings {
            let program_timing = self.per_program_timings.entry(*id).or_default();
            program_timing.accumulated_us = program_timing
                .accumulated_us
                .saturating_add(other.accumulated_us);
            program_timing.accumulated_units = program_timing
                .accumulated_units
                .saturating_add(other.accumulated_units);
            program_timing.count = program_timing.count.saturating_add(other.count);
        }
    }
    pub fn accumulate_program(&mut self, program_id: &Pubkey, us: u64, units: u64) {
        let program_timing = self.per_program_timings.entry(*program_id).or_default();
        program_timing.accumulated_us = program_timing.accumulated_us.saturating_add(us);
        program_timing.accumulated_units = program_timing.accumulated_units.saturating_add(units);
        program_timing.count = program_timing.count.saturating_add(1);
    }
}

// The relevant state of an account before an Instruction executes, used
// to verify account integrity after the Instruction completes
#[derive(Clone, Debug, Default)]
pub struct PreAccount {
    key: Pubkey,
    account: Rc<RefCell<AccountSharedData>>,
    changed: bool,
}
impl PreAccount {
    pub fn new(key: &Pubkey, account: &AccountSharedData) -> Self {
        Self {
            key: *key,
            account: Rc::new(RefCell::new(account.clone())),
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
        do_support_realloc: bool,
    ) -> Result<(), InstructionError> {
        let pre = self.account.borrow();

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

        let data_len_changed = if do_support_realloc {
            // Account data size cannot exceed a maxumum length
            if post.data().len() > MAX_PERMITTED_DATA_LENGTH as usize {
                return Err(InstructionError::InvalidRealloc);
            }

            // The owner of the account can change the size of the data
            let data_len_changed = pre.data().len() != post.data().len();
            if data_len_changed && program_id != pre.owner() {
                return Err(InstructionError::AccountDataSizeChanged);
            }
            data_len_changed
        } else {
            // Only the system program can change the size of the data
            //  and only if the system program owns the account
            let data_len_changed = pre.data().len() != post.data().len();
            if data_len_changed
                && (!system_program::check_id(program_id) // line coverage used to get branch coverage
                    || !system_program::check_id(pre.owner()))
            {
                return Err(InstructionError::AccountDataSizeChanged);
            }
            data_len_changed
        };

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
            timings.total_account_count += 1;
            timings.total_data_size += post.data().len();
            if owner_changed
                || lamports_changed
                || data_len_changed
                || executable_changed
                || rent_epoch_changed
                || self.changed
            {
                timings.changed_account_count += 1;
                timings.data_size_changed += post.data().len();
            }
        }

        Ok(())
    }

    pub fn update(&mut self, account: &AccountSharedData) {
        let mut pre = self.account.borrow_mut();
        let rent_epoch = pre.rent_epoch();
        *pre = account.clone();
        pre.set_rent_epoch(rent_epoch);

        self.changed = true;
    }

    pub fn key(&self) -> &Pubkey {
        &self.key
    }

    pub fn data(&self) -> Ref<[u8]> {
        Ref::map(self.account.borrow(), |account| account.data())
    }

    pub fn lamports(&self) -> u64 {
        self.account.borrow().lamports()
    }

    pub fn executable(&self) -> bool {
        self.account.borrow().executable()
    }

    pub fn is_zeroed(buf: &[u8]) -> bool {
        const ZEROS_LEN: usize = 1024;
        static ZEROS: [u8; ZEROS_LEN] = [0; ZEROS_LEN];
        let mut chunks = buf.chunks_exact(ZEROS_LEN);

        chunks.all(|chunk| chunk == &ZEROS[..])
            && chunks.remainder() == &ZEROS[..chunks.remainder().len()]
    }
}

#[derive(Deserialize, Serialize)]
pub struct InstructionProcessor {
    #[serde(skip)]
    programs: Vec<(Pubkey, ProcessInstructionWithContext)>,
    #[serde(skip)]
    native_loader: NativeLoader,
}

impl std::fmt::Debug for InstructionProcessor {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        #[derive(Debug)]
        struct MessageProcessor<'a> {
            programs: Vec<String>,
            native_loader: &'a NativeLoader,
        }

        // These are just type aliases for work around of Debug-ing above pointers
        type ErasedProcessInstructionWithContext = fn(
            &'static Pubkey,
            &'static [u8],
            &'static mut dyn InvokeContext,
        ) -> Result<(), InstructionError>;

        // rustc doesn't compile due to bug without this work around
        // https://github.com/rust-lang/rust/issues/50280
        // https://users.rust-lang.org/t/display-function-pointer/17073/2
        let processor = MessageProcessor {
            programs: self
                .programs
                .iter()
                .map(|(pubkey, instruction)| {
                    let erased_instruction: ErasedProcessInstructionWithContext = *instruction;
                    format!("{}: {:p}", pubkey, erased_instruction)
                })
                .collect::<Vec<_>>(),
            native_loader: &self.native_loader,
        };

        write!(f, "{:?}", processor)
    }
}

impl Default for InstructionProcessor {
    fn default() -> Self {
        Self {
            programs: vec![],
            native_loader: NativeLoader::default(),
        }
    }
}
impl Clone for InstructionProcessor {
    fn clone(&self) -> Self {
        InstructionProcessor {
            programs: self.programs.clone(),
            native_loader: NativeLoader::default(),
        }
    }
}

#[cfg(RUSTC_WITH_SPECIALIZATION)]
impl ::solana_frozen_abi::abi_example::AbiExample for InstructionProcessor {
    fn example() -> Self {
        // MessageProcessor's fields are #[serde(skip)]-ed and not Serialize
        // so, just rely on Default anyway.
        InstructionProcessor::default()
    }
}

impl InstructionProcessor {
    pub fn programs(&self) -> &[(Pubkey, ProcessInstructionWithContext)] {
        &self.programs
    }

    /// Add a static entrypoint to intercept instructions before the dynamic loader.
    pub fn add_program(
        &mut self,
        program_id: &Pubkey,
        process_instruction: ProcessInstructionWithContext,
    ) {
        match self.programs.iter_mut().find(|(key, _)| program_id == key) {
            Some((_, processor)) => *processor = process_instruction,
            None => self.programs.push((*program_id, process_instruction)),
        }
    }

    /// Remove a program
    pub fn remove_program(&mut self, program_id: &Pubkey) {
        if let Some(position) = self.programs.iter().position(|(key, _)| program_id == key) {
            self.programs.remove(position);
        }
    }

    /// Process an instruction
    /// This method calls the instruction's program entrypoint method
    pub fn process_instruction(
        &self,
        program_id: &Pubkey,
        instruction_data: &[u8],
        invoke_context: &mut dyn InvokeContext,
    ) -> Result<(), InstructionError> {
        if let Some(root_account) = invoke_context.get_keyed_accounts()?.iter().next() {
            let root_id = root_account.unsigned_key();
            if solana_sdk::native_loader::check_id(&root_account.owner()?) {
                for (id, process_instruction) in &self.programs {
                    if id == root_id {
                        invoke_context.remove_first_keyed_account()?;
                        // Call the builtin program
                        return process_instruction(program_id, instruction_data, invoke_context);
                    }
                }
                // Call the program via the native loader
                return self.native_loader.process_instruction(
                    &solana_sdk::native_loader::id(),
                    instruction_data,
                    invoke_context,
                );
            } else {
                let owner_id = &root_account.owner()?;
                for (id, process_instruction) in &self.programs {
                    if id == owner_id {
                        // Call the program via a builtin loader
                        return process_instruction(program_id, instruction_data, invoke_context);
                    }
                }
            }
        }
        Err(InstructionError::UnsupportedProgramId)
    }

    pub fn create_message(
        instruction: &Instruction,
        signers: &[Pubkey],
        invoke_context: &RefMut<&mut dyn InvokeContext>,
    ) -> Result<(Message, Vec<bool>, Vec<usize>), InstructionError> {
        let message = Message::new(&[instruction.clone()], None);

        // Gather keyed_accounts in the order of message.account_keys
        let caller_keyed_accounts = invoke_context.get_keyed_accounts()?;
        let callee_keyed_accounts = message
            .account_keys
            .iter()
            .map(|account_key| {
                caller_keyed_accounts
                    .iter()
                    .find(|keyed_account| keyed_account.unsigned_key() == account_key)
                    .ok_or_else(|| {
                        ic_msg!(
                            *invoke_context,
                            "Instruction references an unknown account {}",
                            account_key
                        );
                        InstructionError::MissingAccount
                    })
            })
            .collect::<Result<Vec<_>, InstructionError>>()?;

        // Check for privilege escalation
        for account in instruction.accounts.iter() {
            let keyed_account = callee_keyed_accounts
                .iter()
                .find_map(|keyed_account| {
                    if &account.pubkey == keyed_account.unsigned_key() {
                        Some(keyed_account)
                    } else {
                        None
                    }
                })
                .ok_or_else(|| {
                    ic_msg!(
                        invoke_context,
                        "Instruction references an unknown account {}",
                        account.pubkey
                    );
                    InstructionError::MissingAccount
                })?;
            // Readonly account cannot become writable
            if account.is_writable && !keyed_account.is_writable() {
                ic_msg!(
                    invoke_context,
                    "{}'s writable privilege escalated",
                    account.pubkey
                );
                return Err(InstructionError::PrivilegeEscalation);
            }

            if account.is_signer && // If message indicates account is signed
            !( // one of the following needs to be true:
                keyed_account.signer_key().is_some() // Signed in the parent instruction
                || signers.contains(&account.pubkey) // Signed by the program
            ) {
                ic_msg!(
                    invoke_context,
                    "{}'s signer privilege escalated",
                    account.pubkey
                );
                return Err(InstructionError::PrivilegeEscalation);
            }
        }
        let caller_write_privileges = callee_keyed_accounts
            .iter()
            .map(|keyed_account| keyed_account.is_writable())
            .collect::<Vec<bool>>();

        // Find and validate executables / program accounts
        let callee_program_id = instruction.program_id;
        let (program_account_index, program_account) = callee_keyed_accounts
            .iter()
            .find(|keyed_account| &callee_program_id == keyed_account.unsigned_key())
            .and_then(|_keyed_account| invoke_context.get_account(&callee_program_id))
            .ok_or_else(|| {
                ic_msg!(invoke_context, "Unknown program {}", callee_program_id);
                InstructionError::MissingAccount
            })?;
        if !program_account.borrow().executable() {
            ic_msg!(
                invoke_context,
                "Account {} is not executable",
                callee_program_id
            );
            return Err(InstructionError::AccountNotExecutable);
        }
        let mut program_indices = vec![program_account_index];
        if program_account.borrow().owner() == &bpf_loader_upgradeable::id() {
            if let UpgradeableLoaderState::Program {
                programdata_address,
            } = program_account.borrow().state()?
            {
                if let Some((programdata_account_index, _programdata_account)) =
                    invoke_context.get_account(&programdata_address)
                {
                    program_indices.push(programdata_account_index);
                } else {
                    ic_msg!(
                        invoke_context,
                        "Unknown upgradeable programdata account {}",
                        programdata_address,
                    );
                    return Err(InstructionError::MissingAccount);
                }
            } else {
                ic_msg!(
                    invoke_context,
                    "Invalid upgradeable program account {}",
                    callee_program_id,
                );
                return Err(InstructionError::MissingAccount);
            }
        }

        Ok((message, caller_write_privileges, program_indices))
    }

    /// Entrypoint for a cross-program invocation from a native program
    pub fn native_invoke(
        invoke_context: &mut dyn InvokeContext,
        instruction: Instruction,
        keyed_account_indices_obsolete: &[usize],
        signers: &[Pubkey],
    ) -> Result<(), InstructionError> {
        let do_support_realloc = invoke_context.is_feature_active(&do_support_realloc::id());
        let invoke_context = RefCell::new(invoke_context);
        let mut invoke_context = invoke_context.borrow_mut();

        // Translate and verify caller's data
        let (message, mut caller_write_privileges, program_indices) =
            Self::create_message(&instruction, signers, &invoke_context)?;
        if !invoke_context.is_feature_active(&fix_write_privs::id()) {
            let caller_keyed_accounts = invoke_context.get_keyed_accounts()?;
            caller_write_privileges = Vec::with_capacity(1 + keyed_account_indices_obsolete.len());
            caller_write_privileges.push(false);
            for index in keyed_account_indices_obsolete.iter() {
                caller_write_privileges.push(caller_keyed_accounts[*index].is_writable());
            }
        };
        let mut account_indices = Vec::with_capacity(message.account_keys.len());
        let mut accounts = Vec::with_capacity(message.account_keys.len());
        for account_key in message.account_keys.iter() {
            let (account_index, account) = invoke_context
                .get_account(account_key)
                .ok_or(InstructionError::MissingAccount)?;
            let account_length = account.borrow().data().len();
            account_indices.push(account_index);
            accounts.push((account, account_length));
        }

        // Record the instruction
        invoke_context.record_instruction(&instruction);

        // Process instruction
        InstructionProcessor::process_cross_program_instruction(
            &message,
            &program_indices,
            &account_indices,
            &caller_write_privileges,
            *invoke_context,
        )?;

        // Verify the called program has not misbehaved
        for (account, prev_size) in accounts.iter() {
            if !do_support_realloc && *prev_size != account.borrow().data().len() && *prev_size != 0
            {
                // Only support for `CreateAccount` at this time.
                // Need a way to limit total realloc size across multiple CPI calls
                ic_msg!(
                    invoke_context,
                    "Inner instructions do not support realloc, only SystemProgram::CreateAccount",
                );
                return Err(InstructionError::InvalidRealloc);
            }
        }

        Ok(())
    }

    /// Process a cross-program instruction
    /// This method calls the instruction's program entrypoint function
    pub fn process_cross_program_instruction(
        message: &Message,
        program_indices: &[usize],
        account_indices: &[usize],
        caller_write_privileges: &[bool],
        invoke_context: &mut dyn InvokeContext,
    ) -> Result<(), InstructionError> {
        // This function is always called with a valid instruction, if that changes return an error
        let instruction = message
            .instructions
            .get(0)
            .ok_or(InstructionError::GenericError)?;

        let program_id = instruction.program_id(&message.account_keys);

        // Verify the calling program hasn't misbehaved
        invoke_context.verify_and_update(instruction, account_indices, caller_write_privileges)?;

        // clear the return data
        invoke_context.set_return_data(None);

        // Invoke callee
        invoke_context.push(
            program_id,
            message,
            instruction,
            program_indices,
            account_indices,
        )?;

        let mut instruction_processor = InstructionProcessor::default();
        for (program_id, process_instruction) in invoke_context.get_programs().iter() {
            instruction_processor.add_program(program_id, *process_instruction);
        }

        let mut result = instruction_processor.process_instruction(
            program_id,
            &instruction.data,
            invoke_context,
        );
        if result.is_ok() {
            // Verify the called program has not misbehaved
            let demote_program_write_locks =
                invoke_context.is_feature_active(&demote_program_write_locks::id());
            let write_privileges: Vec<bool> = (0..message.account_keys.len())
                .map(|i| message.is_writable(i, demote_program_write_locks))
                .collect();
            result =
                invoke_context.verify_and_update(instruction, account_indices, &write_privileges);
        }

        // Restore previous state
        invoke_context.pop();
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::{account::Account, instruction::InstructionError, system_program};

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
                    &AccountSharedData::from(Account {
                        owner: *owner,
                        lamports: std::u64::MAX,
                        data: vec![],
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
            self.pre.account.borrow_mut().set_executable(pre);
            self.post.set_executable(post);
            self
        }
        pub fn lamports(mut self, pre: u64, post: u64) -> Self {
            self.pre.account.borrow_mut().set_lamports(pre);
            self.post.set_lamports(post);
            self
        }
        pub fn owner(mut self, post: &Pubkey) -> Self {
            self.post.set_owner(*post);
            self
        }
        pub fn data(mut self, pre: Vec<u8>, post: Vec<u8>) -> Self {
            self.pre.account.borrow_mut().set_data(pre);
            self.post.set_data(post);
            self
        }
        pub fn rent_epoch(mut self, pre: u64, post: u64) -> Self {
            self.pre.account.borrow_mut().set_rent_epoch(pre);
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
                true,
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

    #[test]
    fn test_debug() {
        let mut instruction_processor = InstructionProcessor::default();
        #[allow(clippy::unnecessary_wraps)]
        fn mock_process_instruction(
            _program_id: &Pubkey,
            _data: &[u8],
            _invoke_context: &mut dyn InvokeContext,
        ) -> Result<(), InstructionError> {
            Ok(())
        }
        #[allow(clippy::unnecessary_wraps)]
        fn mock_ix_processor(
            _pubkey: &Pubkey,
            _data: &[u8],
            _context: &mut dyn InvokeContext,
        ) -> Result<(), InstructionError> {
            Ok(())
        }
        let program_id = solana_sdk::pubkey::new_rand();
        instruction_processor.add_program(&program_id, mock_process_instruction);
        instruction_processor.add_program(&program_id, mock_ix_processor);

        assert!(!format!("{:?}", instruction_processor).is_empty());
    }
}
