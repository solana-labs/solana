//! The `Instructions` struct is a workaround for the lack of Vec<T> support in wasm-bindgen
//! (ref: https://github.com/rustwasm/wasm-bindgen/issues/111)
#![cfg(target_arch = "wasm32")]
use {crate::instruction::Instruction, wasm_bindgen::prelude::*};

#[wasm_bindgen]
#[derive(Default)]
pub struct Instructions {
    instructions: Vec<Instruction>,
}

#[wasm_bindgen]
impl Instructions {
    #[wasm_bindgen(constructor)]
    pub fn constructor() -> Instructions {
        Instructions::default()
    }

    pub fn push(&mut self, instruction: Instruction) {
        self.instructions.push(instruction);
    }
}

impl From<Instructions> for Vec<Instruction> {
    fn from(instructions: Instructions) -> Self {
        instructions.instructions
    }
}
