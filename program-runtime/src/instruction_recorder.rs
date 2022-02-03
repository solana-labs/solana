use {
    solana_sdk::{
        instruction::{CompiledInstruction, Instruction},
        message::SanitizedMessage,
    },
    std::{cell::RefCell, rc::Rc},
};

/// Records and compiles cross-program invoked instructions
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InstructionRecorder {
    inner: Rc<RefCell<Vec<(usize, Instruction)>>>,
}

impl InstructionRecorder {
    pub fn compile_instructions(
        &self,
        message: &SanitizedMessage,
    ) -> Option<Vec<CompiledInstruction>> {
        self.inner
            .borrow()
            .iter()
            .skip(1)
            .map(|(_, ix)| message.try_compile_instruction(ix))
            .collect()
    }

    pub fn record_instruction(&self, stack_height: usize, instruction: Instruction) {
        self.inner.borrow_mut().push((stack_height, instruction));
    }

    pub fn get(&self, index: usize) -> Option<Instruction> {
        self.inner
            .borrow()
            .get(index)
            .map(|(_, instruction)| instruction.clone())
    }

    pub fn find(&self, stack_height: usize, index: usize) -> Option<Instruction> {
        let mut current_index = 0;
        self.inner
            .borrow()
            .iter()
            .rev()
            .skip(1)
            .find(|(this_stack_height, _)| {
                if stack_height == *this_stack_height {
                    if index == current_index {
                        return true;
                    } else {
                        current_index = current_index.saturating_add(1);
                    }
                }
                false
            })
            .map(|(_, instruction)| instruction.clone())
    }
}
