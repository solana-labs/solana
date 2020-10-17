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

use crate::evm::error::DisassemblyError;
use std::io::{self, Cursor, Read};

pub fn assemble(disassembly: Vec<Instruction>) -> Vec<u8> {
    let mut result = Vec::new();
    for disas in disassembly {
        result.extend(assemble_instruction(disas));
    }
    result
}

pub fn assemble_instruction(instruction: Instruction) -> Vec<u8> {
    match instruction {
        Instruction::Stop => vec![0x00],
        Instruction::Add => vec![0x01],
        Instruction::Mul => vec![0x02],
        Instruction::Sub => vec![0x03],
        Instruction::Div => vec![0x04],
        Instruction::SDiv => vec![0x05],
        Instruction::Mod => vec![0x06],
        Instruction::SMod => vec![0x07],
        Instruction::AddMod => vec![0x08],
        Instruction::MulMod => vec![0x09],
        Instruction::Exp => vec![0x0a],
        Instruction::SignExtend => vec![0x0b],
        Instruction::Lt => vec![0x10],
        Instruction::Gt => vec![0x11],
        Instruction::SLt => vec![0x12],
        Instruction::SGt => vec![0x13],
        Instruction::EQ => vec![0x14],
        Instruction::IsZero => vec![0x15],
        Instruction::And => vec![0x16],
        Instruction::Or => vec![0x17],
        Instruction::Xor => vec![0x18],
        Instruction::Not => vec![0x19],
        Instruction::Byte => vec![0x1a],
        Instruction::Shl => vec![0x1b],
        Instruction::Shr => vec![0x1c],
        Instruction::Sar => vec![0x1d],
        Instruction::Sha3 => vec![0x20],
        Instruction::Addr => vec![0x30],
        Instruction::Balance => vec![0x31],
        Instruction::Origin => vec![0x32],
        Instruction::Caller => vec![0x33],
        Instruction::CallValue => vec![0x34],
        Instruction::CallDataLoad => vec![0x35],
        Instruction::CallDataSize => vec![0x36],
        Instruction::CallDataCopy => vec![0x37],
        Instruction::CodeSize => vec![0x38],
        Instruction::CodeCopy => vec![0x39],
        Instruction::GasPrice => vec![0x3a],
        Instruction::ExtCodeSize => vec![0x3b],
        Instruction::ExtCodeCopy => vec![0x3c],
        Instruction::ExtCodeHash => vec![0x3f],
        Instruction::ReturnDataSize => vec![0x3d],
        Instruction::ReturnDataCopy => vec![0x3e],
        Instruction::Blockhash => vec![0x40],
        Instruction::Coinbase => vec![0x41],
        Instruction::Timestamp => vec![0x42],
        Instruction::Number => vec![0x43],
        Instruction::Difficulty => vec![0x44],
        Instruction::GasLimit => vec![0x45],
        Instruction::ChainId => vec![0x46],
        Instruction::Pop => vec![0x50],
        Instruction::MLoad => vec![0x51],
        Instruction::MStore => vec![0x52],
        Instruction::MStore8 => vec![0x53],
        Instruction::SLoad => vec![0x54],
        Instruction::SStore => vec![0x55],
        Instruction::Jump => vec![0x56],
        Instruction::JumpIf => vec![0x57],
        Instruction::PC => vec![0x58],
        Instruction::MSize => vec![0x59],
        Instruction::Gas => vec![0x5a],
        Instruction::JumpDest => vec![0x5b],
        Instruction::Push(v) => {
            let mut res = vec![0x60 + (v.len() as u8 - 1)];
            res.extend(v);
            res
        }
        Instruction::Dup(v) => vec![0x80 + v as u8],
        Instruction::Swap(v) => vec![0x90 + (v as u8 - 1)],
        Instruction::Log(v) => vec![0xa0 + v as u8],
        Instruction::Create => vec![0xf0],
        Instruction::Call => vec![0xf1],
        Instruction::CallCode => vec![0xf2],
        Instruction::Return => vec![0xf3],
        Instruction::DelegateCall => vec![0xf4],
        Instruction::Create2 => vec![0xfb],
        Instruction::Revert => vec![0xfd],
        Instruction::StaticCall => vec![0xfa],
        Instruction::SelfDestruct => vec![0xff],
        Instruction::Invalid => vec![0xfe],
    }
}

pub fn disassemble_next_byte(
    cursor: &mut Cursor<&[u8]>,
) -> Result<(usize, Instruction), DisassemblyError> {
    let offset = cursor.position() as usize;
    let opcode = read_n_bytes(cursor, 1)?[0];
    let instruction = match opcode {
        0x00 => Instruction::Stop,
        0x01 => Instruction::Add,
        0x02 => Instruction::Mul,
        0x03 => Instruction::Sub,
        0x04 => Instruction::Div,
        0x05 => Instruction::SDiv,
        0x06 => Instruction::Mod,
        0x07 => Instruction::SMod,
        0x08 => Instruction::AddMod,
        0x09 => Instruction::MulMod,
        0x0a => Instruction::Exp,
        0x0b => Instruction::SignExtend,
        0x10 => Instruction::Lt,
        0x11 => Instruction::Gt,
        0x12 => Instruction::SLt,
        0x13 => Instruction::SGt,
        0x14 => Instruction::EQ,
        0x15 => Instruction::IsZero,
        0x16 => Instruction::And,
        0x17 => Instruction::Or,
        0x18 => Instruction::Xor,
        0x19 => Instruction::Not,
        0x1a => Instruction::Byte,
        0x1b => Instruction::Shl,
        0x1c => Instruction::Shr,
        0x1d => Instruction::Sar,
        0x20 => Instruction::Sha3,
        0x30 => Instruction::Addr,
        0x31 => Instruction::Balance,
        0x32 => Instruction::Origin,
        0x33 => Instruction::Caller,
        0x34 => Instruction::CallValue,
        0x35 => Instruction::CallDataLoad,
        0x36 => Instruction::CallDataSize,
        0x37 => Instruction::CallDataCopy,
        0x38 => Instruction::CodeSize,
        0x39 => Instruction::CodeCopy,
        0x3a => Instruction::GasPrice,
        0x3b => Instruction::ExtCodeSize,
        0x3c => Instruction::ExtCodeCopy,
        0x3f => Instruction::ExtCodeHash,
        0x3d => Instruction::ReturnDataSize,
        0x3e => Instruction::ReturnDataCopy,
        0x40 => Instruction::Blockhash,
        0x41 => Instruction::Coinbase,
        0x42 => Instruction::Timestamp,
        0x43 => Instruction::Number,
        0x44 => Instruction::Difficulty,
        0x45 => Instruction::GasLimit,
        0x46 => Instruction::ChainId,
        0x50 => Instruction::Pop,
        0x51 => Instruction::MLoad,
        0x52 => Instruction::MStore,
        0x53 => Instruction::MStore8,
        0x54 => Instruction::SLoad,
        0x55 => Instruction::SStore,
        0x56 => Instruction::Jump,
        0x57 => Instruction::JumpIf,
        0x58 => Instruction::PC,
        0x59 => Instruction::MSize,
        0x5a => Instruction::Gas,
        0x5b => Instruction::JumpDest,
        0x60 | 0x61 | 0x62 | 0x63 | 0x64 | 0x65 | 0x66 | 0x67 | 0x68 | 0x69 | 0x6a | 0x6b
        | 0x6c | 0x6d | 0x6e | 0x6f => match read_n_bytes(cursor, 1 + (opcode & 0x0f) as usize) {
            Err(..) => return Err(DisassemblyError::TooFewBytesForPush),
            Ok(v) => Instruction::Push(v),
        },
        0x70 | 0x71 | 0x72 | 0x73 | 0x74 | 0x75 | 0x76 | 0x77 | 0x78 | 0x79 | 0x7a | 0x7b
        | 0x7c | 0x7d | 0x7e | 0x7f => {
            match read_n_bytes(cursor, (0x11 + (opcode & 0x0f)) as usize) {
                Err(..) => return Err(DisassemblyError::TooFewBytesForPush),
                Ok(v) => Instruction::Push(v),
            }
        }
        0x80 => Instruction::Dup(0),
        0x81 => Instruction::Dup(1),
        0x82 => Instruction::Dup(2),
        0x83 => Instruction::Dup(3),
        0x84 => Instruction::Dup(4),
        0x85 => Instruction::Dup(5),
        0x86 => Instruction::Dup(6),
        0x87 => Instruction::Dup(7),
        0x88 => Instruction::Dup(8),
        0x89 => Instruction::Dup(9),
        0x8a => Instruction::Dup(10),
        0x8b => Instruction::Dup(11),
        0x8c => Instruction::Dup(12),
        0x8d => Instruction::Dup(13),
        0x8e => Instruction::Dup(14),
        0x8f => Instruction::Dup(15),
        0x90 => Instruction::Swap(1),
        0x91 => Instruction::Swap(2),
        0x92 => Instruction::Swap(3),
        0x93 => Instruction::Swap(4),
        0x94 => Instruction::Swap(5),
        0x95 => Instruction::Swap(6),
        0x96 => Instruction::Swap(7),
        0x97 => Instruction::Swap(8),
        0x98 => Instruction::Swap(9),
        0x99 => Instruction::Swap(10),
        0x9a => Instruction::Swap(11),
        0x9b => Instruction::Swap(12),
        0x9c => Instruction::Swap(13),
        0x9d => Instruction::Swap(14),
        0x9e => Instruction::Swap(15),
        0x9f => Instruction::Swap(16),
        0xa0 => Instruction::Log(0),
        0xa1 => Instruction::Log(1),
        0xa2 => Instruction::Log(2),
        0xa3 => Instruction::Log(3),
        0xa4 => Instruction::Log(4),
        0xf0 => Instruction::Create,
        0xf1 => Instruction::Call,
        0xf2 => Instruction::CallCode,
        0xf3 => Instruction::Return,
        0xf4 => Instruction::DelegateCall,
        0xfb => Instruction::Create2,
        0xfd => Instruction::Revert,
        0xfa => Instruction::StaticCall,
        0xff => Instruction::SelfDestruct,
        0xfe | _ => Instruction::Invalid,
    };
    Ok((offset, instruction))
}

fn read_n_bytes(cursor: &mut Cursor<&[u8]>, n: usize) -> Result<Vec<u8>, io::Error> {
    let mut buffer = vec![0; n];
    cursor.read_exact(&mut buffer)?;
    Ok(buffer)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Instruction {
    Stop,
    Add,
    Mul,
    Sub,
    Div,
    SDiv,
    Mod,
    SMod,
    AddMod,
    MulMod,
    Exp,
    SignExtend,
    Lt,
    Gt,
    SLt,
    SGt,
    EQ,
    IsZero,
    And,
    Or,
    Xor,
    Not,
    Byte,
    Shl,
    Shr, // logical shift right
    Sar, // arithmetic shift right
    Sha3,
    Addr,
    Balance,
    Origin,
    Caller,
    CallValue,
    CallDataLoad,
    CallDataSize,
    CallDataCopy,
    CodeSize,
    CodeCopy,
    GasPrice,
    ExtCodeSize,
    ExtCodeCopy,
    ReturnDataSize,
    ReturnDataCopy,
    ExtCodeHash,
    Blockhash,
    Coinbase,
    Timestamp,
    Number,
    Difficulty,
    GasLimit,
    ChainId,
    Pop,
    MLoad,
    MStore,
    MStore8,
    SLoad,
    SStore,
    Jump,
    JumpIf,
    PC,
    MSize,
    Gas,
    JumpDest,
    Push(Vec<u8>),
    Dup(usize),
    Swap(usize),
    Log(usize),
    Create,
    Call,
    CallCode,
    Return,
    DelegateCall,
    Create2,
    Revert,
    StaticCall,
    Invalid,
    SelfDestruct,
}

impl Instruction {
    pub fn size(&self) -> usize {
        match self {
            Self::Push(ref a) => a.len() + 1,
            _ => 1,
        }
    }

    pub fn writes_to_memory(&self) -> bool {
        match self {
            Self::MStore
            | Self::MStore8
            | Self::CallDataCopy
            | Self::CodeCopy
            | Self::ExtCodeCopy
            | Self::ReturnDataCopy
            | Self::Call
            | Self::StaticCall
            | Self::DelegateCall
            | Self::CallCode => true,
            _ => false,
        }
    }

    pub fn reads_from_memory(&self) -> bool {
        match self {
            Self::MLoad
            | Self::Create
            | Self::Call
            | Self::StaticCall
            | Self::DelegateCall
            | Self::CallCode
            | Self::Return
            | Self::Revert => true,
            _ => false,
        }
    }

    pub fn writes_to_storage(&self) -> bool {
        match self {
            Self::SStore => true,
            _ => false,
        }
    }

    pub fn reads_from_storage(&self) -> bool {
        match self {
            Self::SLoad => true,
            _ => false,
        }
    }

    pub fn halts_execution(&self) -> bool {
        match self {
            Self::Return | Self::Stop | Self::Invalid | Self::SelfDestruct | Self::Revert => true,
            _ => false,
        }
    }

    pub fn ends_basic_block(&self) -> bool {
        match self {
            Self::Return
            | Self::Stop
            | Self::Invalid
            | Self::SelfDestruct
            | Self::Revert
            | Self::Jump
            | Self::JumpIf => true,
            _ => false,
        }
    }

    pub fn pops(&self) -> usize {
        match self {
            Self::Stop
            | Self::Addr
            | Self::Origin
            | Self::Caller
            | Self::CallValue
            | Self::CallDataSize
            | Self::CodeSize
            | Self::GasPrice
            | Self::ChainId
            | Self::Coinbase
            | Self::Timestamp
            | Self::Number
            | Self::Difficulty
            | Self::GasLimit
            | Self::PC
            | Self::MSize
            | Self::Gas
            | Self::JumpDest
            | Self::Push(..)
            | Self::Invalid
            | Self::ReturnDataSize => 0,
            Self::IsZero
            | Self::Not
            | Self::Balance
            | Self::CallDataLoad
            | Self::ExtCodeSize
            | Self::Blockhash
            | Self::Pop
            | Self::MLoad
            | Self::SLoad
            | Self::Jump
            | Self::SelfDestruct
            | Self::ExtCodeHash => 1,
            Self::Add
            | Self::Mul
            | Self::Sub
            | Self::Div
            | Self::SDiv
            | Self::Mod
            | Self::SMod
            | Self::Exp
            | Self::SignExtend
            | Self::Lt
            | Self::Gt
            | Self::SLt
            | Self::SGt
            | Self::EQ
            | Self::And
            | Self::Or
            | Self::Xor
            | Self::Byte
            | Self::Sha3
            | Self::MStore
            | Self::MStore8
            | Self::SStore
            | Self::JumpIf
            | Self::Return
            | Self::Revert
            | Self::Shl
            | Self::Shr
            | Self::Sar => 2,
            Self::AddMod
            | Self::MulMod
            | Self::CallDataCopy
            | Self::CodeCopy
            | Self::Create
            | Self::ReturnDataCopy
            | Self::Create2 => 3,
            Self::ExtCodeCopy => 4,
            Self::DelegateCall | Self::StaticCall => 6,
            Self::Call | Self::CallCode => 7,
            Self::Dup(u) => u + 1,
            Self::Swap(u) | Self::Log(u) => u + 2,
        }
    }

    pub fn pushes(&self) -> usize {
        match self {
            Self::Stop
            | Self::CallDataCopy
            | Self::CodeCopy
            | Self::ExtCodeCopy
            | Self::Pop
            | Self::MStore
            | Self::MStore8
            | Self::SStore
            | Self::Jump
            | Self::JumpIf
            | Self::JumpDest
            | Self::Push(..)
            | Self::Log(..)
            | Self::Return
            | Self::Invalid
            | Self::SelfDestruct
            | Self::ReturnDataCopy
            | Self::Revert => 0,
            Self::Dup(u) | Self::Swap(u) => u + 2,
            _ => 1,
        }
    }
}
