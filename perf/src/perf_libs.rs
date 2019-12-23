use core::ffi::c_void;
use dlopen::symbor::{Container, SymBorApi, Symbol};
use dlopen_derive::SymBorApi;
use log::*;
use solana_sdk::packet::Packet;
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::os::raw::{c_int, c_uint};
use std::path::{Path, PathBuf};
use std::sync::Once;

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

    #[allow(clippy::type_complexity)]
    pub ed25519_sign_many: Symbol<
        'a,
        unsafe extern "C" fn(
            vecs: *mut Elems,
            num: u32,          //number of vecs
            message_size: u32, //size of each element inside the elems field of the vec
            total_packets: u32,
            total_signatures: u32,
            message_lens: *const u32,
            pubkey_offsets: *const u32,
            privkey_offsets: *const u32,
            signed_message_offsets: *const u32,
            sgnatures_out: *mut u8, //combined length of all the items in vecs
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

enum GpuApi {
    Cuda,
    OpenCL,
}

static mut GPU_API: Option<GpuApi> = None;

#[derive(PartialEq)]
pub enum PerfFeature {
    Ed25519Verify,
    Ed25519Sign,
    PohVerify,
    Chacha,
}

pub fn is_supported(feature: PerfFeature) -> bool {
    unsafe {
        match &GPU_API {
            Some(GpuApi::OpenCL) => feature == PerfFeature::Ed25519Verify,
            _ => true,
        }
    }
}

fn init(name: &OsStr, gpu_api: GpuApi) {
    static INIT_HOOK: Once = Once::new();

    info!("Loading {:?}", name);
    unsafe {
        INIT_HOOK.call_once(|| {
            GPU_API = Some(gpu_api);
            API = Some(Container::load(name).unwrap_or_else(|err| {
                error!("Unable to load {:?}: {}", name, err);
                std::process::exit(1);
            }));
        })
    }
}

fn locate_perf_libs() -> Option<PathBuf> {
    let exe = env::current_exe().expect("Unable to get executable path");
    let perf_libs = exe.parent().unwrap().join("perf-libs");
    if perf_libs.is_dir() {
        info!("perf-libs found at {:?}", perf_libs);
        return Some(perf_libs);
    }
    warn!("{:?} does not exist", perf_libs);
    None
}

fn find_cuda_home(perf_libs_path: &Path) -> Option<PathBuf> {
    // Search /usr/local for a `cuda-` directory that matches a perf-libs subdirectory
    for entry in fs::read_dir(&perf_libs_path).unwrap() {
        if let Ok(entry) = entry {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let dir_name = path.file_name().unwrap().to_str().unwrap_or("");
            if !dir_name.starts_with("cuda-") {
                continue;
            }

            let cuda_home: PathBuf = ["/", "usr", "local", dir_name].iter().collect();
            if !cuda_home.is_dir() {
                continue;
            }

            return Some(cuda_home);
        }
    }
    None
}

pub fn init_cuda() {
    if let Some(perf_libs_path) = locate_perf_libs() {
        if let Some(cuda_home) = find_cuda_home(&perf_libs_path) {
            info!("CUDA installation found at {:?}", cuda_home);

            let cuda_lib64_dir = cuda_home.join("lib64");
            if cuda_lib64_dir.is_dir() {
                let ld_library_path = cuda_lib64_dir.to_str().unwrap_or("").to_string()
                    + ":"
                    + &env::var("LD_LIBRARY_PATH").unwrap_or_else(|_| "".to_string());
                info!("LD_LIBRARY_PATH set to {:?}", ld_library_path);

                // Prefix LD_LIBRARY_PATH with $CUDA_HOME/lib64 directory
                // to ensure the correct CUDA version is used
                env::set_var("LD_LIBRARY_PATH", ld_library_path)
            } else {
                warn!("{:?} does not exist", cuda_lib64_dir);
            }

            let libcuda_crypt = perf_libs_path
                .join(cuda_home.file_name().unwrap())
                .join("libcuda-crypt.so");
            return init(libcuda_crypt.as_os_str(), GpuApi::Cuda);
        } else {
            warn!("CUDA installation not found");
        }
    }

    // Last resort!  Blindly load the shared object and hope it all works out
    init(OsStr::new("libcuda-crypt.so"), GpuApi::Cuda)
}

pub fn init_opencl() {
    if let Some(perf_libs_path) = locate_perf_libs() {
        let ld_library_path = perf_libs_path
            .join("opencl")
            .to_str()
            .unwrap_or("")
            .to_string()
            + ":"
            + &env::var("LD_LIBRARY_PATH").unwrap_or_else(|_| "".to_string());
        info!("LD_LIBRARY_PATH set to {:?}", ld_library_path);
        env::set_var("LD_LIBRARY_PATH", ld_library_path);

        let libcl_crypt = perf_libs_path.join("opencl").join("libcl-crypt.so");

        return init(libcl_crypt.as_os_str(), GpuApi::OpenCL);
    }

    init(OsStr::new("libcl-crypt.so"), GpuApi::OpenCL)
}

pub fn api() -> Option<&'static Container<Api<'static>>> {
    {
        static INIT_HOOK: Once = Once::new();
        INIT_HOOK.call_once(|| {
            if std::env::var("TEST_PERF_LIBS_CUDA").is_ok() {
                init_cuda();
            } else if std::env::var("TEST_PERF_LIBS_OPENCL").is_ok() {
                init_opencl();
            }
        })
    }

    unsafe { API.as_ref() }
}
