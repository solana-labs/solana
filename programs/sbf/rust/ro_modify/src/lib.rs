#![cfg(target_os = "solana")]

use solana_program::{
    account_info::AccountInfo, entrypoint::ProgramResult, msg, program::invoke,
    program_error::ProgramError, pubkey::Pubkey, syscalls::sol_invoke_signed_c, system_instruction,
};

#[derive(Debug)]
#[repr(C)]
struct SolInstruction {
    program_id_addr: u64,
    accounts_addr: u64,
    accounts_len: usize,
    data_addr: u64,
    data_len: usize,
}

/// Rust representation of C's SolAccountMeta
#[derive(Debug)]
#[repr(C)]
struct SolAccountMeta {
    pubkey_addr: u64,
    is_writable: bool,
    is_signer: bool,
}

/// Rust representation of C's SolAccountInfo
#[derive(Debug, Clone)]
#[repr(C)]
struct SolAccountInfo {
    key_addr: u64,
    lamports_addr: u64,
    data_len: u64,
    data_addr: u64,
    owner_addr: u64,
    rent_epoch: u64,
    is_signer: bool,
    is_writable: bool,
    executable: bool,
}

/// Rust representation of C's SolSignerSeed
#[derive(Debug)]
#[repr(C)]
struct SolSignerSeedC {
    addr: u64,
    len: u64,
}

/// Rust representation of C's SolSignerSeeds
#[derive(Debug)]
#[repr(C)]
struct SolSignerSeedsC {
    addr: u64,
    len: u64,
}

const READONLY_ACCOUNTS: &[SolAccountInfo] = &[
    SolAccountInfo {
        is_signer: false,
        is_writable: false,
        executable: true,
        key_addr: 0x400000010,
        owner_addr: 0x400000030,
        lamports_addr: 0x400000050,
        rent_epoch: 0,
        data_addr: 0x400000060,
        data_len: 14,
    },
    SolAccountInfo {
        is_signer: true,
        is_writable: true,
        executable: false,
        key_addr: 0x400002880,
        owner_addr: 0x4000028A0,
        lamports_addr: 0x4000028c0,
        rent_epoch: 0,
        data_addr: 0x4000028d0,
        data_len: 0,
    },
];

const PUBKEY: Pubkey = Pubkey::new_from_array([
    0_u8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0,
]);

fn check_preconditions(
    in_infos: &[AccountInfo],
    static_infos: &[SolAccountInfo],
) -> Result<(), ProgramError> {
    for (in_info, static_info) in in_infos.iter().zip(static_infos) {
        check!(in_info.key.as_ref().as_ptr() as u64, static_info.key_addr);
        check!(
            in_info.owner.as_ref().as_ptr() as u64,
            static_info.owner_addr
        );
        check!(
            unsafe { *in_info.lamports.as_ptr() as *const u64 as u64 },
            static_info.lamports_addr
        );
        check!(
            in_info.try_borrow_data()?.as_ptr() as u64,
            static_info.data_addr
        );
        check!(in_info.data_len() as u64, static_info.data_len);
    }
    Ok(())
}

solana_program::entrypoint!(process_instruction);
fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    check_preconditions(accounts, READONLY_ACCOUNTS)?;

    match instruction_data[0] {
        1 => {
            let system_instruction = system_instruction::allocate(accounts[1].key, 42);
            let metas = &[SolAccountMeta {
                is_signer: true,
                is_writable: true,
                pubkey_addr: accounts[1].key as *const _ as u64,
            }];
            let instruction = SolInstruction {
                accounts_addr: metas.as_ptr() as u64,
                accounts_len: metas.len(),
                data_addr: system_instruction.data.as_ptr() as u64,
                data_len: system_instruction.data.len(),
                program_id_addr: accounts[0].key as *const Pubkey as u64,
            };
            unsafe {
                check!(
                    0,
                    sol_invoke_signed_c(
                        &instruction as *const _ as *const _,
                        READONLY_ACCOUNTS.as_ptr() as *const _,
                        READONLY_ACCOUNTS.len() as u64,
                        std::ptr::null(),
                        0,
                    )
                );
            }
            let ptr = &READONLY_ACCOUNTS[1].data_len as *const _ as u64 as *mut u64;
            check!(42, unsafe { read_val(ptr) });
        }
        2 => {
            // Not sure how to get a const data length in an Rc<RefCell<&mut [u8]>>
        }
        3 => {
            let mut new_accounts =
                &mut [READONLY_ACCOUNTS[0].clone(), READONLY_ACCOUNTS[1].clone()];
            new_accounts[1].owner_addr = &PUBKEY as *const _ as u64;
            let system_instruction = system_instruction::assign(accounts[1].key, program_id);
            let metas = &[SolAccountMeta {
                is_signer: true,
                is_writable: true,
                pubkey_addr: accounts[1].key as *const _ as u64,
            }];
            let instruction = SolInstruction {
                accounts_addr: metas.as_ptr() as u64,
                accounts_len: metas.len(),
                data_addr: system_instruction.data.as_ptr() as u64,
                data_len: system_instruction.data.len(),
                program_id_addr: accounts[0].key as *const Pubkey as u64,
            };
            unsafe {
                check!(
                    0,
                    sol_invoke_signed_c(
                        &instruction as *const _ as *const _,
                        new_accounts.as_ptr() as *const _,
                        new_accounts.len() as u64,
                        std::ptr::null(),
                        0,
                    )
                );
            }
        }
        4 => {
            let mut new_account = accounts[1].clone();
            new_account.owner = &PUBKEY;
            let instruction = system_instruction::assign(accounts[1].key, program_id);
            invoke(&instruction, &[accounts[0].clone(), new_account])?;
        }
        _ => check!(0, 1),
    }

    Ok(())
}

#[macro_export]
macro_rules! check {
    ($left:expr, $right:expr) => {
        if $left != $right {
            msg!(
                "Condition failure: {:?} != {:?} at line {:?}",
                $left,
                $right,
                line!()
            );
            return Err(ProgramError::Custom(0));
        }
    };
}

/// Skirt the compiler and force a read from a const value
/// # Safety
#[inline(never)]
pub unsafe fn read_val<T: Copy>(ptr: *mut T) -> T {
    *ptr
}
