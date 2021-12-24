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
    records: Vec<Vec<CompiledInstruction>>,
}

impl InstructionRecorder {
    pub fn new_ref(instructions_in_message: usize) -> Rc<RefCell<Self>> {
        Rc::new(RefCell::new(Self {
            inner: Vec::with_capacity(instructions_in_message),
            records: Vec::with_capacity(instructions_in_message),
        }))
    }

    pub fn compile_instructions(
        &self,
        message: &SanitizedMessage,
    ) -> Option<Vec<Vec<CompiledInstruction>>> {
        let result: Option<Vec<Vec<CompiledInstruction>>> = self
            .inner
            .iter()
            .map(|instructions| {
                instructions
                    .iter()
                    .map(|ix| message.try_compile_instruction(ix))
                    .collect()
            })
            .collect();
        assert_eq!(result.as_ref().unwrap(), &self.records);
        result
    }

    pub fn begin_next_recording(&mut self) {
        self.inner.push(Vec::new());
        self.records.push(Vec::new());
    }

    pub fn record_instruction(&mut self, instruction: Instruction) {
        if let Some(inner) = self.inner.last_mut() {
            inner.push(instruction);
        }
    }

    pub fn record_compiled_instruction(&mut self, instruction: CompiledInstruction) {
        if let Some(records) = self.records.last_mut() {
            records.push(instruction);
        }
    }
}
