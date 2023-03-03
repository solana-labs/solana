//! Example Rust-based SBF program that issues a cross-program-invocation

use {
    core::slice,
    std::{mem, ptr},
};

pub enum BenchInstruction {
    Nop,
    ReadAccounts(ReadAccounts),
    WriteAccounts(WriteAccounts),
    Recurse(Recurse),
}

impl BenchInstruction {
    pub fn tag(&self) -> u8 {
        match self {
            BenchInstruction::Nop => 0,
            BenchInstruction::ReadAccounts(_) => 1,
            BenchInstruction::WriteAccounts(_) => 2,
            BenchInstruction::Recurse(_) => 3,
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = vec![self.tag()];
        match self {
            BenchInstruction::Nop => {}
            BenchInstruction::ReadAccounts(read_accounts) => bytes.extend_from_slice(unsafe {
                slice::from_raw_parts(
                    read_accounts as *const _ as *const u8,
                    mem::size_of::<ReadAccounts>(),
                )
            }),
            BenchInstruction::WriteAccounts(write_accounts) => bytes.extend_from_slice(unsafe {
                slice::from_raw_parts(
                    write_accounts as *const _ as *const u8,
                    mem::size_of::<WriteAccounts>(),
                )
            }),
            BenchInstruction::Recurse(recurse) => bytes.extend_from_slice(unsafe {
                slice::from_raw_parts(recurse as *const _ as *const u8, mem::size_of::<Recurse>())
            }),
        }
        bytes
    }
}

impl TryFrom<&[u8]> for BenchInstruction {
    type Error = ();

    fn try_from(input: &[u8]) -> Result<Self, Self::Error> {
        let (tag, rest) = input.split_first().ok_or(())?;
        match tag {
            0 => Ok(BenchInstruction::Nop),
            1 => {
                let read_accounts =
                    unsafe { ptr::read_unaligned(rest.as_ptr() as *const ReadAccounts) };
                Ok(BenchInstruction::ReadAccounts(read_accounts))
            }
            2 => {
                let write_accounts =
                    unsafe { ptr::read_unaligned(rest.as_ptr() as *const WriteAccounts) };
                Ok(BenchInstruction::WriteAccounts(write_accounts))
            }
            3 => {
                let recurse = unsafe { ptr::read_unaligned(rest.as_ptr() as *const Recurse) };
                Ok(BenchInstruction::Recurse(recurse))
            }
            _ => Err(()),
        }
    }
}

#[repr(C)]
pub struct ReadAccounts {
    pub num_accounts: usize,
    pub size: usize,
}

#[repr(C)]
pub struct WriteAccounts {
    pub num_accounts: usize,
    pub size: usize,
}

#[repr(C)]
pub struct Recurse {
    pub n: usize,
}
