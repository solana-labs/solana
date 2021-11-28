use std::{cell::RefCell, rc::Rc};

use solana_sdk::{
    instruction::{CompiledInstruction, Instruction},
    message::SanitizedMessage,
};

/// Records and compiles cross-program invoked instructions
#[derive(Clone, Default)]
pub struct InstructionRecorder {
    inner: Rc<RefCell<Vec<Instruction>>>,
}

impl InstructionRecorder {
    pub fn compile_instructions(
        &self,
        message: &SanitizedMessage,
    ) -> Option<Vec<CompiledInstruction>> {
        self.inner
            .borrow()
            .iter()
            .map(|ix| message.try_compile_instruction(ix))
            .collect()
    }

    pub fn record_instruction(&self, instruction: Instruction) {
        self.inner.borrow_mut().push(instruction);
    }
}
