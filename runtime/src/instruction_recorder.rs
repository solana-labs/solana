use std::{cell::RefCell, rc::Rc};

use solana_sdk::instruction::CompiledInstruction;

/// Records and compiles cross-program invoked instructions
#[derive(Clone, Default)]
pub struct InstructionRecorder {
    inner: Rc<RefCell<Vec<CompiledInstruction>>>,
}

impl InstructionRecorder {
    pub(crate) fn into_inner(self) -> Vec<CompiledInstruction> {
        std::mem::take(&mut self.inner.borrow_mut())
    }

    pub fn record_instruction(&self, instruction: CompiledInstruction) {
        self.inner.borrow_mut().push(instruction);
    }
}
