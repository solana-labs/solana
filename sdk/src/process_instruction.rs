use solana_sdk::{
    account::Account,
    feature_set::{
        bpf_compute_budget_balancing, max_invoke_depth_4, max_program_call_depth_64,
        pubkey_log_syscall_enabled, FeatureSet,
    },
    instruction::{CompiledInstruction, Instruction, InstructionError},
    keyed_account::KeyedAccount,
    message::Message,
    pubkey::Pubkey,
};
use std::{cell::RefCell, fmt::Debug, rc::Rc, sync::Arc};

// Prototype of a native loader entry point
///
/// program_id: Program ID of the currently executing program
/// keyed_accounts: Accounts passed as part of the instruction
/// instruction_data: Instruction data
/// invoke_context: Invocation context
pub type LoaderEntrypoint = unsafe extern "C" fn(
    program_id: &Pubkey,
    keyed_accounts: &[KeyedAccount],
    instruction_data: &[u8],
    invoke_context: &dyn InvokeContext,
) -> Result<(), InstructionError>;

pub type ProcessInstructionWithContext =
    fn(&Pubkey, &[KeyedAccount], &[u8], &mut dyn InvokeContext) -> Result<(), InstructionError>;

/// Invocation context passed to loaders
pub trait InvokeContext {
    /// Push a program ID on to the invocation stack
    fn push(&mut self, key: &Pubkey) -> Result<(), InstructionError>;
    /// Pop a program ID off of the invocation stack
    fn pop(&mut self);
    /// Verify and update PreAccount state based on program execution
    fn verify_and_update(
        &mut self,
        message: &Message,
        instruction: &CompiledInstruction,
        accounts: &[Rc<RefCell<Account>>],
    ) -> Result<(), InstructionError>;
    /// Get the program ID of the currently executing program
    fn get_caller(&self) -> Result<&Pubkey, InstructionError>;
    /// Get a list of built-in programs
    fn get_programs(&self) -> &[(Pubkey, ProcessInstructionWithContext)];
    /// Get this invocation's logger
    fn get_logger(&self) -> Rc<RefCell<dyn Logger>>;
    /// Get this invocation's compute budget
    fn get_bpf_compute_budget(&self) -> &BpfComputeBudget;
    /// Get this invocation's compute meter
    fn get_compute_meter(&self) -> Rc<RefCell<dyn ComputeMeter>>;
    /// Loaders may need to do work in order to execute a program.  Cache
    /// the work that can be re-used across executions
    fn add_executor(&mut self, pubkey: &Pubkey, executor: Arc<dyn Executor>);
    /// Get the completed loader work that can be re-used across executions
    fn get_executor(&mut self, pubkey: &Pubkey) -> Option<Arc<dyn Executor>>;
    /// Record invoked instruction
    fn record_instruction(&self, instruction: &Instruction);
    /// Get the bank's active feature set
    fn is_feature_active(&self, feature_id: &Pubkey) -> bool;
}

#[derive(Clone, Copy, Debug, AbiExample)]
pub struct BpfComputeBudget {
    /// Number of compute units that an instruction is allowed.  Compute units
    /// are consumed by program execution, resources they use, etc...
    pub max_units: u64,
    /// Number of compute units consumed by a log call
    pub log_units: u64,
    /// Number of compute units consumed by a log_u64 call
    pub log_64_units: u64,
    /// Number of compute units consumed by a create_program_address call
    pub create_program_address_units: u64,
    /// Number of compute units consumed by an invoke call (not including the cost incurred by
    /// the called program)
    pub invoke_units: u64,
    /// Maximum cross-program invocation depth allowed including the original caller
    pub max_invoke_depth: usize,
    /// Base number of compute units consumed to call SHA256
    pub sha256_base_cost: u64,
    /// Incremental number of units consumed by SHA256 (based on bytes)
    pub sha256_byte_cost: u64,
    /// Maximum BPF to BPF call depth
    pub max_call_depth: usize,
    /// Size of a stack frame in bytes, must match the size specified in the LLVM BPF backend
    pub stack_frame_size: usize,
    /// Number of compute units consumed by logging a `Pubkey`
    pub log_pubkey_units: u64,
}
impl Default for BpfComputeBudget {
    fn default() -> Self {
        Self::new(&FeatureSet::all_enabled())
    }
}
impl BpfComputeBudget {
    pub fn new(feature_set: &FeatureSet) -> Self {
        let mut bpf_compute_budget =
        // Original
        BpfComputeBudget {
            max_units: 100_000,
            log_units: 0,
            log_64_units: 0,
            create_program_address_units: 0,
            invoke_units: 0,
            max_invoke_depth: 1,
            sha256_base_cost: 85,
            sha256_byte_cost: 1,
            max_call_depth: 20,
            stack_frame_size: 4_096,
            log_pubkey_units: 0,
        };

        if feature_set.is_active(&bpf_compute_budget_balancing::id()) {
            bpf_compute_budget = BpfComputeBudget {
                max_units: 200_000,
                log_units: 100,
                log_64_units: 100,
                create_program_address_units: 1500,
                invoke_units: 1000,
                ..bpf_compute_budget
            };
        }
        if feature_set.is_active(&max_invoke_depth_4::id()) {
            bpf_compute_budget = BpfComputeBudget {
                max_invoke_depth: 4,
                ..bpf_compute_budget
            };
        }

        if feature_set.is_active(&max_program_call_depth_64::id()) {
            bpf_compute_budget = BpfComputeBudget {
                max_call_depth: 64,
                ..bpf_compute_budget
            };
        }
        if feature_set.is_active(&pubkey_log_syscall_enabled::id()) {
            bpf_compute_budget = BpfComputeBudget {
                log_pubkey_units: 100,
                ..bpf_compute_budget
            };
        }
        bpf_compute_budget
    }
}

/// Compute meter
pub trait ComputeMeter {
    /// Consume compute units
    fn consume(&mut self, amount: u64) -> Result<(), InstructionError>;
    /// Get the number of remaining compute units
    fn get_remaining(&self) -> u64;
}

/// Log messages
pub trait Logger {
    fn log_enabled(&self) -> bool;
    /// Log a message
    fn log(&mut self, message: &str);
}

/// Program executor
pub trait Executor: Debug + Send + Sync {
    /// Execute the program
    fn execute(
        &self,
        program_id: &Pubkey,
        keyed_accounts: &[KeyedAccount],
        instruction_data: &[u8],
        invoke_context: &mut dyn InvokeContext,
    ) -> Result<(), InstructionError>;
}

#[derive(Debug, Default, Clone)]
pub struct MockComputeMeter {
    pub remaining: u64,
}
impl ComputeMeter for MockComputeMeter {
    fn consume(&mut self, amount: u64) -> Result<(), InstructionError> {
        let exceeded = self.remaining < amount;
        self.remaining = self.remaining.saturating_sub(amount);
        if exceeded {
            return Err(InstructionError::ComputationalBudgetExceeded);
        }
        Ok(())
    }
    fn get_remaining(&self) -> u64 {
        self.remaining
    }
}

#[derive(Debug, Default, Clone)]
pub struct MockLogger {
    pub log: Rc<RefCell<Vec<String>>>,
}
impl Logger for MockLogger {
    fn log_enabled(&self) -> bool {
        true
    }
    fn log(&mut self, message: &str) {
        self.log.borrow_mut().push(message.to_string());
    }
}

#[derive(Debug)]
pub struct MockInvokeContext {
    pub key: Pubkey,
    pub logger: MockLogger,
    pub bpf_compute_budget: BpfComputeBudget,
    pub compute_meter: MockComputeMeter,
}
impl Default for MockInvokeContext {
    fn default() -> Self {
        MockInvokeContext {
            key: Pubkey::default(),
            logger: MockLogger::default(),
            bpf_compute_budget: BpfComputeBudget::default(),
            compute_meter: MockComputeMeter {
                remaining: std::i64::MAX as u64,
            },
        }
    }
}
impl InvokeContext for MockInvokeContext {
    fn push(&mut self, _key: &Pubkey) -> Result<(), InstructionError> {
        Ok(())
    }
    fn pop(&mut self) {}
    fn verify_and_update(
        &mut self,
        _message: &Message,
        _instruction: &CompiledInstruction,
        _accounts: &[Rc<RefCell<Account>>],
    ) -> Result<(), InstructionError> {
        Ok(())
    }
    fn get_caller(&self) -> Result<&Pubkey, InstructionError> {
        Ok(&self.key)
    }
    fn get_programs(&self) -> &[(Pubkey, ProcessInstructionWithContext)] {
        &[]
    }
    fn get_logger(&self) -> Rc<RefCell<dyn Logger>> {
        Rc::new(RefCell::new(self.logger.clone()))
    }
    fn get_bpf_compute_budget(&self) -> &BpfComputeBudget {
        &self.bpf_compute_budget
    }
    fn get_compute_meter(&self) -> Rc<RefCell<dyn ComputeMeter>> {
        Rc::new(RefCell::new(self.compute_meter.clone()))
    }
    fn add_executor(&mut self, _pubkey: &Pubkey, _executor: Arc<dyn Executor>) {}
    fn get_executor(&mut self, _pubkey: &Pubkey) -> Option<Arc<dyn Executor>> {
        None
    }
    fn record_instruction(&self, _instruction: &Instruction) {}
    fn is_feature_active(&self, _feature_id: &Pubkey) -> bool {
        true
    }
}
