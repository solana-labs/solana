use crate::packet::Packet;
use core::ffi::c_void;
use dlopen::symbor::{Container, SymBorApi, Symbol};
use std::os::raw::{c_int, c_uint};

#[repr(C)]
pub struct Elems {
    pub elems: *const Packet,
    pub num: u32,
}

#[derive(SymBorApi)]
pub struct Api<'a> {
    pub ed25519_init: Symbol<'a, unsafe extern "C" fn() -> bool>,
    pub ed25519_set_verbose: Symbol<'a, unsafe extern "C" fn(val: bool)>,

    #[allow(clippy::type_complexity)]
    pub ed25519_verify_many: Symbol<
        'a,
        unsafe extern "C" fn(
            vecs: *const Elems,
            num: u32,          //number of vecs
            message_size: u32, //size of each element inside the elems field of the vec
            total_packets: u32,
            total_signatures: u32,
            message_lens: *const u32,
            pubkey_offsets: *const u32,
            signature_offsets: *const u32,
            signed_message_offsets: *const u32,
            out: *mut u8, //combined length of all the items in vecs
            use_non_default_stream: u8,
        ) -> u32,
    >,

    pub chacha_cbc_encrypt_many_sample: Symbol<
        'a,
        unsafe extern "C" fn(
            input: *const u8,
            sha_state: *mut u8,
            in_len: usize,
            keys: *const u8,
            ivec: *mut u8,
            num_keys: u32,
            samples: *const u64,
            num_samples: u32,
            starting_block: u64,
            time_us: *mut f32,
        ),
    >,

    pub chacha_init_sha_state: Symbol<'a, unsafe extern "C" fn(sha_state: *mut u8, num_keys: u32)>,
    pub chacha_end_sha_state:
        Symbol<'a, unsafe extern "C" fn(sha_state_in: *const u8, out: *mut u8, num_keys: u32)>,

    pub poh_verify_many: Symbol<
        'a,
        unsafe extern "C" fn(
            hashes: *mut u8,
            num_hashes_arr: *const u64,
            num_elems: usize,
            use_non_default_stream: u8,
        ) -> c_int,
    >,

    pub cuda_host_register:
        Symbol<'a, unsafe extern "C" fn(ptr: *mut c_void, size: usize, flags: c_uint) -> c_int>,

    pub cuda_host_unregister: Symbol<'a, unsafe extern "C" fn(ptr: *mut c_void) -> c_int>,
}

static mut API: Option<Container<Api>> = None;

fn init(name: &str) {
    use std::sync::Once;
    static INIT_HOOK: Once = Once::new();

    unsafe {
        INIT_HOOK.call_once(|| {
            let api = Container::load(name).unwrap_or_else(|err| {
                eprintln!("Error: unable to load {}: {}", name, err);
                std::process::exit(1);
            });
            API = Some(api);
        })
    }
}

pub fn init_cuda() {
    init("libcuda-crypt.so")
}

pub fn api() -> Option<&'static Container<Api<'static>>> {
    #[cfg(test)]
    {
        if std::env::var("TEST_PERF_LIBS_CUDA").is_ok() {
            init_cuda();
        }
    }

    unsafe { API.as_ref() }
}
