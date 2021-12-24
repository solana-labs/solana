use {
    solana_sdk::{
        instruction::{CompiledInstruction, Instruction},
        message::SanitizedMessage,
    },
    std::{cell::RefCell, rc::Rc},
};

/// Records and compiles cross-program invoked instructions
#[derive(Clone)]
pub struct InstructionRecorder {
    inner: Vec<Vec<Instruction>>,
}

impl InstructionRecorder {
    pub fn new_ref(instructions_in_message: usize) -> Rc<RefCell<Self>> {
        Rc::new(RefCell::new(Self {
            inner: Vec::with_capacity(instructions_in_message),
        }))
    }

    pub fn compile_instructions(
        &self,
        message: &SanitizedMessage,
    ) -> Option<Vec<Vec<CompiledInstruction>>> {
        self.inner
            .iter()
            .map(|instructions| {
                instructions
                    .iter()
                    .map(|ix| message.try_compile_instruction(ix))
                    .collect()
            })
            .collect()
    }

    pub fn begin_next_recording(&mut self) {
        self.inner.push(Vec::new());
    }

    pub fn record_instruction(&mut self, instruction: Instruction) {
        if let Some(inner) = self.inner.last_mut() {
            inner.push(instruction);
        }
    }
}
