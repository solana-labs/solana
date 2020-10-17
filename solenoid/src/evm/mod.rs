// Copyright 2019 Joel Frank
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// 	https://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

pub mod error;
pub mod instructions;

use uint::rustc_hex::FromHex;
use std::io::Cursor;

use instructions::{assemble_instruction, disassemble_next_byte};

pub use error::DisassemblyError;
pub use instructions::Instruction;

type InstrTy = (usize, Instruction);

#[derive(Clone, Debug)]
pub struct Disassembly {
    pub instructions: Vec<InstrTy>,
}

impl Disassembly {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DisassemblyError> {
        let instructions = disassemble_bytes(bytes)?;
        Ok(Self { instructions })
    }

    pub fn from_hex_str(input: &str) -> Result<Self, DisassemblyError> {
        let instructions = disassemble_hex_str(input)?;
        Ok(Self { instructions })
    }
}

pub fn assemble_instructions(disassembly: Vec<Instruction>) -> Vec<u8> {
    let mut result = Vec::new();
    for disas in disassembly {
        result.extend(assemble_instruction(disas));
    }
    result
}

fn disassemble_hex_str(input: &str) -> Result<Vec<InstrTy>, DisassemblyError> {
    let input = if input[0..2] == *"0x" {
        &input[2..]
    } else {
        input
    };
    let bytes = (input).from_hex::<Vec<_>>()?;
    disassemble_bytes(&bytes)
}

fn disassemble_bytes(bytes: &[u8]) -> Result<Vec<InstrTy>, DisassemblyError> {
    let mut instructions = Vec::new();
    let mut cursor = Cursor::new(bytes);
    loop {
        let result = disassemble_next_byte(&mut cursor);
        match result {
            Err(DisassemblyError::IOError(..)) => break,
            Ok((offset, instruction)) => {
                instructions.push((offset, instruction));
            }
            Err(err) => {
                if let DisassemblyError::TooFewBytesForPush = err {
                    // the solidity compiler sometimes puts push instructions at the end, however,
                    // this is considered normal behaviour
                    break;
                }
                return Err(err);
            }
        }
    }

    Ok(instructions)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hex::FromHex;

    #[test]
    fn test_parse() {
        let code = "608060405234801561001057600080fd5b506040516101403803806101408339818101604052602081101561003357600080fd5b8101908080519060200190929190505050806000806101000a81548160ff0219169083151502179055505060d48061006c6000396000f3fe6080604052348015600f57600080fd5b506004361060325760003560e01c80636d4ce63c146037578063cde4efa9146057575b600080fd5b603d605f565b604051808215151515815260200191505060405180910390f35b605d6075565b005b60008060009054906101000a900460ff16905090565b6000809054906101000a900460ff16156000806101000a81548160ff02191690831515021790555056fea265627a7a7231582082bcd0833ba0da688a9423e31c3ac40adacca43eb13e585f36ef1dd07e14c45864736f6c63430005110032";
        let bytes: Vec<u8> = Vec::from_hex(code).expect("Invalid Hex String");
        for opcode in Disassembly::from_bytes(&bytes).unwrap().instructions {
            println!("{:?}", opcode);
        }
    }
}