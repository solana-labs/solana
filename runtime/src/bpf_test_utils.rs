use crate::process_instruction::{
    ComputeBudget, ComputeMeter, Executor, InvokeContext, Logger, ProcessInstruction,
};
use solana_sdk::{
    account::Account, instruction::CompiledInstruction, instruction::Instruction,
    instruction::InstructionError, message::Message, pubkey::Pubkey,
};
use std::{cell::RefCell, rc::Rc, sync::Arc};

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
    pub compute_budget: ComputeBudget,
    pub compute_meter: MockComputeMeter,
}
impl Default for MockInvokeContext {
    fn default() -> Self {
        MockInvokeContext {
            key: Pubkey::default(),
            logger: MockLogger::default(),
            compute_budget: ComputeBudget::default(),
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
    fn get_programs(&self) -> &[(Pubkey, ProcessInstruction)] {
        &[]
    }
    fn get_logger(&self) -> Rc<RefCell<dyn Logger>> {
        Rc::new(RefCell::new(self.logger.clone()))
    }
    fn get_compute_budget(&self) -> &ComputeBudget {
        &self.compute_budget
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
