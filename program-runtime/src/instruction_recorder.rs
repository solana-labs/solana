use {
    solana_sdk::instruction::CompiledInstruction,
    std::{cell::RefCell, rc::Rc},
};

/// Records and compiles cross-program invoked instructions
#[derive(Clone)]
pub struct InstructionRecorder {
    records: Vec<Vec<CompiledInstruction>>,
}

impl InstructionRecorder {
    pub fn new_ref(instructions_in_message: usize) -> Rc<RefCell<Self>> {
        Rc::new(RefCell::new(Self {
            records: Vec::with_capacity(instructions_in_message),
        }))
    }

    pub fn deconstruct(self) -> Vec<Vec<CompiledInstruction>> {
        self.records
    }

    pub fn begin_next_recording(&mut self) {
        self.records.push(Vec::new());
    }

    pub fn record_compiled_instruction(&mut self, instruction: CompiledInstruction) {
        if let Some(records) = self.records.last_mut() {
            records.push(instruction);
        }
    }
}
