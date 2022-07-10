use {
    crate::{
        syscalls::{translate_slice, translate_slice_mut, translate_type, translate_type_mut},
        BpfError,
    },
    solana_program_runtime::invoke_context::InvokeContext,
    solana_rbpf::{error::EbpfError, memory_region::MemoryMapping},
    solana_sdk::{instruction::AccountMeta as NativeAccountMeta, pubkey::Pubkey},
    std::marker::PhantomData,
};

/// Stable representation of a `Vec<T>` in the SBFv2 Rust runtime.
/// Only supports the global allocator.
// Size: 0x18 (24 bytes)
#[repr(C)]
pub(crate) struct StdVec<T: Sized> {
    pub(crate) ptr: u64,     // 0x00 (8 bytes)
    pub(crate) raw_cap: u64, // 0x08 (8 bytes)
    pub(crate) raw_len: u64, // 0x10 (8 bytes)
    _phantom: PhantomData<T>,
}

impl<T: Sized> StdVec<T> {
    /// Returns a slice to the elements pointed to by the Vec object.
    ///
    /// Panics on 32-bit hosts if the `raw_len` parameter exceeds 2^31.
    pub(crate) fn translate(
        &self,
        memory_mapping: &MemoryMapping,
        invoke_context: &InvokeContext,
    ) -> Result<&[T], EbpfError<BpfError>> {
        translate_slice(
            memory_mapping,
            self.ptr,
            self.raw_len,
            invoke_context.get_check_aligned(),
            invoke_context.get_check_size(),
        )
    }
}

/// Stable representation of a `solana_program::instruction::AccountMeta`
/// in the SBFv2 Rust runtime.
// Size: 0x?? (?? bytes)
#[derive(Clone)]
#[repr(C)]
pub(crate) struct AccountMeta {
    pub(crate) pubkey: Pubkey,
    pub(crate) is_signer: bool,
    pub(crate) is_writable: bool,
}

impl From<&AccountMeta> for NativeAccountMeta {
    fn from(meta: &AccountMeta) -> Self {
        Self {
            pubkey: meta.pubkey,
            is_signer: meta.is_signer,
            is_writable: meta.is_writable,
        }
    }
}

/// Stable representation of a `solana_program::instruction::Instruction`
/// in the SBFv2 Rust runtime.
// Size: 0x50 (80 bytes)
#[repr(C)]
pub(crate) struct Instruction {
    pub(crate) accounts: StdVec<AccountMeta>, // 0x00 (24 bytes)
    pub(crate) data: StdVec<u8>,              // 0x18 (24 bytes)
    pub(crate) program_id: Pubkey,            // 0x30 (32 bytes)
}

#[repr(C)]
pub(crate) struct Ptr<T: Sized> {
    pub(crate) ptr: u64,
    _phantom: PhantomData<T>,
}

impl<T: Sized> Ptr<T> {
    pub(crate) fn new(vm_addr: u64) -> Self {
        Self {
            ptr: vm_addr,
            _phantom: PhantomData::default(),
        }
    }

    pub(crate) fn translate<'a>(
        &self,
        memory_mapping: &MemoryMapping,
        invoke_context: &InvokeContext,
    ) -> Result<&'a T, EbpfError<BpfError>> {
        translate_type(memory_mapping, self.ptr, invoke_context.get_check_aligned())
    }

    pub(crate) fn translate_mut<'a>(
        &self,
        memory_mapping: &MemoryMapping,
        invoke_context: &InvokeContext,
    ) -> Result<&'a mut T, EbpfError<BpfError>> {
        translate_type_mut(memory_mapping, self.ptr, invoke_context.get_check_aligned())
    }
}

#[repr(C)]
pub(crate) struct StdRcBox<T: Sized> {
    _strong: u64,
    _weak: u64,
    pub(crate) value: T,
}

#[repr(C)]
pub(crate) struct StdRefCell<T: Sized> {
    _borrow: i64,
    pub(crate) value: T,
}

#[repr(C)]
pub(crate) struct Slice<T: Sized> {
    pub(crate) ptr: u64,
    pub(crate) len: u64,
    _phantom: PhantomData<T>,
}

impl<T: Sized> Slice<T> {
    pub(crate) fn translate<'a>(
        &self,
        memory_mapping: &MemoryMapping,
        invoke_context: &InvokeContext,
    ) -> Result<&'a [T], EbpfError<BpfError>> {
        translate_slice(
            memory_mapping,
            self.ptr,
            self.len,
            invoke_context.get_check_aligned(),
            invoke_context.get_check_size(),
        )
    }

    pub(crate) fn translate_mut<'a>(
        &self,
        memory_mapping: &MemoryMapping,
        invoke_context: &InvokeContext,
    ) -> Result<&'a mut [T], EbpfError<BpfError>> {
        translate_slice_mut(
            memory_mapping,
            self.ptr,
            self.len,
            invoke_context.get_check_aligned(),
            invoke_context.get_check_size(),
        )
    }
}

/// Stable instruction of a `solana_program::account_info::AccountInfo`
/// in the SBFv2 Rust runtime.
// Size: 0x30 (48 bytes)
#[repr(C)]
pub(crate) struct AccountInfo {
    pub(crate) key: Ptr<Pubkey>, // 0x00 (8 bytes)
    pub(crate) lamports: Ptr<StdRcBox<StdRefCell<Ptr<u64>>>>, // 0x08 (8 bytes)
    pub(crate) data: Ptr<StdRcBox<StdRefCell<Slice<u8>>>>, // 0x10 (8 bytes)
    pub(crate) owner: Ptr<Pubkey>, // 0x18 (8 bytes)
    pub(crate) rent_epoch: u64,  // 0x20 (8 bytes)
    pub(crate) is_signer: bool,  // 0x28 (1 byte)
    pub(crate) is_writable: bool, // 0x29 (1 byte)
    pub(crate) executable: bool, // 0x2a (1 byte)
    _padding: [u8; 5],           // 0x30 (5 bytes)
}
