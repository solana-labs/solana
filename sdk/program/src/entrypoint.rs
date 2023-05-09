//! The Rust-based BPF program entrypoint supported by the latest BPF loader.
//!
//! For more information see the [`bpf_loader`] module.
//!
//! [`bpf_loader`]: crate::bpf_loader

extern crate alloc;
use {
    crate::{account_info::AccountInfo, program_error::ProgramError, pubkey::Pubkey},
    alloc::vec::Vec,
    std::{
        alloc::Layout,
        cell::RefCell,
        mem::size_of,
        ptr::null_mut,
        rc::Rc,
        result::Result as ResultGeneric,
        slice::{from_raw_parts, from_raw_parts_mut},
    },
};

pub type ProgramResult = ResultGeneric<(), ProgramError>;

/// User implemented function to process an instruction
///
/// program_id: Program ID of the currently executing program accounts: Accounts
/// passed as part of the instruction instruction_data: Instruction data
pub type ProcessInstruction =
    fn(program_id: &Pubkey, accounts: &[AccountInfo], instruction_data: &[u8]) -> ProgramResult;

/// Programs indicate success with a return value of 0
pub const SUCCESS: u64 = 0;

/// Start address of the memory region used for program heap.
pub const HEAP_START_ADDRESS: u64 = 0x300000000;
/// Length of the heap memory region used for program heap.
pub const HEAP_LENGTH: usize = 32 * 1024;

/// Value used to indicate that a serialized account is not a duplicate
pub const NON_DUP_MARKER: u8 = u8::MAX;

/// Declare the program entrypoint and set up global handlers.
///
/// This macro emits the common boilerplate necessary to begin program
/// execution, calling a provided function to process the program instruction
/// supplied by the runtime, and reporting its result to the runtime.
///
/// It also sets up a [global allocator] and [panic handler], using the
/// [`custom_heap_default`] and [`custom_panic_default`] macros.
///
/// [`custom_heap_default`]: crate::custom_heap_default
/// [`custom_panic_default`]: crate::custom_panic_default
///
/// [global allocator]: https://doc.rust-lang.org/stable/std/alloc/trait.GlobalAlloc.html
/// [panic handler]: https://doc.rust-lang.org/nomicon/panic-handler.html
///
/// The argument is the name of a function with this type signature:
///
/// ```ignore
/// fn process_instruction(
///     program_id: &Pubkey,      // Public key of the account the program was loaded into
///     accounts: &[AccountInfo], // All accounts required to process the instruction
///     instruction_data: &[u8],  // Serialized instruction-specific data
/// ) -> ProgramResult;
/// ```
///
/// # Cargo features
///
/// This macro emits symbols and definitions that may only be defined once
/// globally. As such, if linked to other Rust crates it will cause compiler
/// errors. To avoid this, it is common for Solana programs to define an
/// optional [Cargo feature] called `no-entrypoint`, and use it to conditionally
/// disable the `entrypoint` macro invocation, as well as the
/// `process_instruction` function. See a typical pattern for this in the
/// example below.
///
/// [Cargo feature]: https://doc.rust-lang.org/cargo/reference/features.html
///
/// The code emitted by this macro can be customized by adding cargo features
/// _to your own crate_ (the one that calls this macro) and enabling them:
///
/// - If the `custom-heap` feature is defined then the macro will not set up the
///   global allocator, allowing `entrypoint` to be used with your own
///   allocator. See documentation for the [`custom_heap_default`] macro for
///   details of customizing the global allocator.
///
/// - If the `custom-panic` feature is defined then the macro will not define a
///   panic handler, allowing `entrypoint` to be used with your own panic
///   handler. See documentation for the [`custom_panic_default`] macro for
///   details of customizing the panic handler.
///
/// # Examples
///
/// Defining an entrypoint and making it conditional on the `no-entrypoint`
/// feature. Although the `entrypoint` module is written inline in this example,
/// it is common to put it into its own file.
///
/// ```no_run
/// #[cfg(not(feature = "no-entrypoint"))]
/// pub mod entrypoint {
///
///     use solana_program::{
///         account_info::AccountInfo,
///         entrypoint,
///         entrypoint::ProgramResult,
///         msg,
///         pubkey::Pubkey,
///     };
///
///     entrypoint!(process_instruction);
///
///     pub fn process_instruction(
///         program_id: &Pubkey,
///         accounts: &[AccountInfo],
///         instruction_data: &[u8],
///     ) -> ProgramResult {
///         msg!("Hello world");
///
///         Ok(())
///     }
///
/// }
/// ```
#[macro_export]
macro_rules! entrypoint {
    ($process_instruction:ident) => {
        /// # Safety
        #[no_mangle]
        pub unsafe extern "C" fn entrypoint(input: *mut u8) -> u64 {
            let (program_id, accounts, instruction_data) =
                unsafe { $crate::entrypoint::deserialize(input) };
            match $process_instruction(&program_id, &accounts, &instruction_data) {
                Ok(()) => $crate::entrypoint::SUCCESS,
                Err(error) => error.into(),
            }
        }
        $crate::custom_heap_default!();
        $crate::custom_panic_default!();
    };
}

/// Define the default global allocator.
///
/// The default global allocator is enabled only if the calling crate has not
/// disabled it using [Cargo features] as described below. It is only defined
/// for [BPF] targets.
///
/// [Cargo features]: https://doc.rust-lang.org/cargo/reference/features.html
/// [BPF]: https://docs.solana.com/developing/on-chain-programs/overview#berkeley-packet-filter-bpf
///
/// # Cargo features
///
/// A crate that calls this macro can provide its own custom heap
/// implementation, or allow others to provide their own custom heap
/// implementation, by adding a `custom-heap` feature to its `Cargo.toml`. After
/// enabling the feature, one may define their own [global allocator] in the
/// standard way.
///
/// [global allocator]: https://doc.rust-lang.org/stable/std/alloc/trait.GlobalAlloc.html
///
#[macro_export]
macro_rules! custom_heap_default {
    () => {
        #[cfg(all(not(feature = "custom-heap"), target_os = "solana"))]
        #[global_allocator]
        static A: $crate::entrypoint::BumpAllocator = $crate::entrypoint::BumpAllocator {
            start: $crate::entrypoint::HEAP_START_ADDRESS as usize,
            len: $crate::entrypoint::HEAP_LENGTH,
        };
    };
}

/// Define the default global panic handler.
///
/// This must be used if the [`entrypoint`] macro is not used, and no other
/// panic handler has been defined; otherwise compilation will fail with a
/// missing `custom_panic` symbol.
///
/// The default global allocator is enabled only if the calling crate has not
/// disabled it using [Cargo features] as described below. It is only defined
/// for [BPF] targets.
///
/// [Cargo features]: https://doc.rust-lang.org/cargo/reference/features.html
/// [BPF]: https://docs.solana.com/developing/on-chain-programs/overview#berkeley-packet-filter-bpf
///
/// # Cargo features
///
/// A crate that calls this macro can provide its own custom panic handler, or
/// allow others to provide their own custom panic handler, by adding a
/// `custom-panic` feature to its `Cargo.toml`. After enabling the feature, one
/// may define their own panic handler.
///
/// A good way to reduce the final size of the program is to provide a
/// `custom_panic` implementation that does nothing. Doing so will cut ~25kb
/// from a noop program. That number goes down the more the programs pulls in
/// Rust's standard library for other purposes.
///
/// # Defining a panic handler for Solana
///
/// _The mechanism for defining a Solana panic handler is different [from most
/// Rust programs][rpanic]._
///
/// [rpanic]: https://doc.rust-lang.org/nomicon/panic-handler.html
///
/// To define a panic handler one must define a `custom_panic` function
/// with the `#[no_mangle]` attribute, as below:
///
/// ```ignore
/// #[cfg(all(feature = "custom-panic", target_os = "solana"))]
/// #[no_mangle]
/// fn custom_panic(info: &core::panic::PanicInfo<'_>) {
///     $crate::msg!("{}", info);
/// }
/// ```
///
/// The above is how Solana defines the default panic handler.
#[macro_export]
macro_rules! custom_panic_default {
    () => {
        #[cfg(all(not(feature = "custom-panic"), target_os = "solana"))]
        #[no_mangle]
        fn custom_panic(info: &core::panic::PanicInfo<'_>) {
            // Full panic reporting
            $crate::msg!("{}", info);
        }
    };
}

/// The bump allocator used as the default rust heap when running programs.
pub struct BumpAllocator {
    pub start: usize,
    pub len: usize,
}
/// Integer arithmetic in this global allocator implementation is safe when
/// operating on the prescribed `HEAP_START_ADDRESS` and `HEAP_LENGTH`. Any
/// other use may overflow and is thus unsupported and at one's own risk.
#[allow(clippy::integer_arithmetic)]
unsafe impl std::alloc::GlobalAlloc for BumpAllocator {
    #[inline]
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pos_ptr = self.start as *mut usize;

        let mut pos = *pos_ptr;
        if pos == 0 {
            // First time, set starting position
            pos = self.start + self.len;
        }
        pos = pos.saturating_sub(layout.size());
        pos &= !(layout.align().wrapping_sub(1));
        if pos < self.start + size_of::<*mut u8>() {
            return null_mut();
        }
        *pos_ptr = pos;
        pos as *mut u8
    }
    #[inline]
    unsafe fn dealloc(&self, _: *mut u8, _: Layout) {
        // I'm a bump allocator, I don't free
    }
}

/// Maximum number of bytes a program may add to an account during a single realloc
pub const MAX_PERMITTED_DATA_INCREASE: usize = 1_024 * 10;

/// `assert_eq(std::mem::align_of::<u128>(), 8)` is true for BPF but not for some host machines
pub const BPF_ALIGN_OF_U128: usize = 8;

/// Deserialize the input arguments
///
/// The integer arithmetic in this method is safe when called on a buffer that was
/// serialized by runtime. Use with buffers serialized otherwise is unsupported and
/// done at one's own risk.
///
/// # Safety
#[allow(clippy::integer_arithmetic)]
#[allow(clippy::type_complexity)]
pub unsafe fn deserialize<'a>(input: *mut u8) -> (&'a Pubkey, Vec<AccountInfo<'a>>, &'a [u8]) {
    let mut offset: usize = 0;

    // Number of accounts present

    #[allow(clippy::cast_ptr_alignment)]
    let num_accounts = *(input.add(offset) as *const u64) as usize;
    offset += size_of::<u64>();

    // Account Infos

    let mut accounts = Vec::with_capacity(num_accounts);
    for _ in 0..num_accounts {
        let dup_info = *(input.add(offset) as *const u8);
        offset += size_of::<u8>();
        if dup_info == NON_DUP_MARKER {
            #[allow(clippy::cast_ptr_alignment)]
            let is_signer = *(input.add(offset) as *const u8) != 0;
            offset += size_of::<u8>();

            #[allow(clippy::cast_ptr_alignment)]
            let is_writable = *(input.add(offset) as *const u8) != 0;
            offset += size_of::<u8>();

            #[allow(clippy::cast_ptr_alignment)]
            let executable = *(input.add(offset) as *const u8) != 0;
            offset += size_of::<u8>();

            // The original data length is stored here because these 4 bytes were
            // originally only used for padding and served as a good location to
            // track the original size of the account data in a compatible way.
            let original_data_len_offset = offset;
            offset += size_of::<u32>();

            let key: &Pubkey = &*(input.add(offset) as *const Pubkey);
            offset += size_of::<Pubkey>();

            let owner: &Pubkey = &*(input.add(offset) as *const Pubkey);
            offset += size_of::<Pubkey>();

            #[allow(clippy::cast_ptr_alignment)]
            let lamports = Rc::new(RefCell::new(&mut *(input.add(offset) as *mut u64)));
            offset += size_of::<u64>();

            #[allow(clippy::cast_ptr_alignment)]
            let data_len = *(input.add(offset) as *const u64) as usize;
            offset += size_of::<u64>();

            // Store the original data length for detecting invalid reallocations and
            // requires that MAX_PERMITTED_DATA_LENGTH fits in a u32
            *(input.add(original_data_len_offset) as *mut u32) = data_len as u32;

            let data = Rc::new(RefCell::new({
                from_raw_parts_mut(input.add(offset), data_len)
            }));
            offset += data_len + MAX_PERMITTED_DATA_INCREASE;
            offset += (offset as *const u8).align_offset(BPF_ALIGN_OF_U128); // padding

            #[allow(clippy::cast_ptr_alignment)]
            let rent_epoch = *(input.add(offset) as *const u64);
            offset += size_of::<u64>();

            accounts.push(AccountInfo {
                key,
                is_signer,
                is_writable,
                lamports,
                data,
                owner,
                executable,
                rent_epoch,
            });
        } else {
            offset += 7; // padding

            // Duplicate account, clone the original
            accounts.push(accounts[dup_info as usize].clone());
        }
    }

    // Instruction data

    #[allow(clippy::cast_ptr_alignment)]
    let instruction_data_len = *(input.add(offset) as *const u64) as usize;
    offset += size_of::<u64>();

    let instruction_data = { from_raw_parts(input.add(offset), instruction_data_len) };
    offset += instruction_data_len;

    // Program Id

    let program_id: &Pubkey = &*(input.add(offset) as *const Pubkey);

    (program_id, accounts, instruction_data)
}

#[cfg(test)]
mod test {
    use {super::*, std::alloc::GlobalAlloc};

    #[test]
    fn test_bump_allocator() {
        // alloc the entire
        {
            let heap = vec![0u8; 128];
            let allocator = BumpAllocator {
                start: heap.as_ptr() as *const _ as usize,
                len: heap.len(),
            };
            for i in 0..128 - size_of::<*mut u8>() {
                let ptr = unsafe {
                    allocator.alloc(Layout::from_size_align(1, size_of::<u8>()).unwrap())
                };
                assert_eq!(
                    ptr as *const _ as usize,
                    heap.as_ptr() as *const _ as usize + heap.len() - 1 - i
                );
            }
            assert_eq!(null_mut(), unsafe {
                allocator.alloc(Layout::from_size_align(1, 1).unwrap())
            });
        }
        // check alignment
        {
            let heap = vec![0u8; 128];
            let allocator = BumpAllocator {
                start: heap.as_ptr() as *const _ as usize,
                len: heap.len(),
            };
            let ptr =
                unsafe { allocator.alloc(Layout::from_size_align(1, size_of::<u8>()).unwrap()) };
            assert_eq!(0, ptr.align_offset(size_of::<u8>()));
            let ptr =
                unsafe { allocator.alloc(Layout::from_size_align(1, size_of::<u16>()).unwrap()) };
            assert_eq!(0, ptr.align_offset(size_of::<u16>()));
            let ptr =
                unsafe { allocator.alloc(Layout::from_size_align(1, size_of::<u32>()).unwrap()) };
            assert_eq!(0, ptr.align_offset(size_of::<u32>()));
            let ptr =
                unsafe { allocator.alloc(Layout::from_size_align(1, size_of::<u64>()).unwrap()) };
            assert_eq!(0, ptr.align_offset(size_of::<u64>()));
            let ptr =
                unsafe { allocator.alloc(Layout::from_size_align(1, size_of::<u128>()).unwrap()) };
            assert_eq!(0, ptr.align_offset(size_of::<u128>()));
            let ptr = unsafe { allocator.alloc(Layout::from_size_align(1, 64).unwrap()) };
            assert_eq!(0, ptr.align_offset(64));
        }
        // alloc entire block (minus the pos ptr)
        {
            let heap = vec![0u8; 128];
            let allocator = BumpAllocator {
                start: heap.as_ptr() as *const _ as usize,
                len: heap.len(),
            };
            let ptr =
                unsafe { allocator.alloc(Layout::from_size_align(120, size_of::<u8>()).unwrap()) };
            assert_ne!(ptr, null_mut());
            assert_eq!(0, ptr.align_offset(size_of::<u64>()));
        }
    }
}
