use {crate::config::sealevel_config_opt::*, solana_rbpf::vm::Config};

/// Sealevel virtual machine config.
#[derive(Default)]
pub struct sealevel_config {
    pub(crate) config: Config,
    pub(crate) no_verify: bool,
}

#[repr(C)]
pub enum sealevel_config_opt {
    SEALEVEL_OPT_NONE,
    SEALEVEL_OPT_NO_VERIFY,
    SEALEVEL_OPT_MAX_CALL_DEPTH,
    SEALEVEL_OPT_STACK_FRAME_SIZE,
    SEALEVEL_OPT_ENABLE_STACK_FRAME_GAPS,
    SEALEVEL_OPT_INSTRUCTION_METER_CHECKPOINT_DISTANCE,
    SEALEVEL_OPT_ENABLE_INSTRUCTION_METER,
    SEALEVEL_OPT_ENABLE_INSTRUCTION_TRACING,
    SEALEVEL_OPT_ENABLE_SYMBOL_AND_SECTION_LABELS,
    SEALEVEL_OPT_DISABLE_UNRESOLVED_SYMBOLS_AT_RUNTIME,
    SEALEVEL_OPT_REJECT_BROKEN_ELFS,
    SEALEVEL_OPT_NOOP_INSTRUCTION_RATIO,
    SEALEVEL_OPT_SANITIZE_USER_PROVIDED_VALUES,
    SEALEVEL_OPT_ENCRYPT_ENVIRONMENT_REGISTERS,
    SEALEVEL_OPT_DISABLE_DEPRECATED_LOAD_INSTRUCTIONS,
    SEALEVEL_OPT_SYSCALL_BPF_FUNCTION_HASH_COLLISION,
    SEALEVEL_OPT_REJECT_CALLX_R10,
    SEALEVEL_OPT_DYNAMIC_STACK_FRAMES,
    SEALEVEL_OPT_ENABLE_SDIV,
    SEALEVEL_OPT_OPTIMIZE_RODATA,
    SEALEVEL_OPT_STATIC_SYSCALLS,
    SEALEVEL_OPT_ENABLE_ELF_VADDR,
}

impl sealevel_config {
    fn new() -> Self {
        Self::default()
    }
}

/// Creates a new Sealevel machine config.
///
/// # Safety
/// Call `sealevel_config_free` on the return value after you are done using it.
/// Failure to do so results in a memory leak.
#[no_mangle]
pub extern "C" fn sealevel_config_new() -> *mut sealevel_config {
    let wrapper = sealevel_config::new();
    Box::into_raw(Box::new(wrapper))
}

/// Sets a config option given the config key and exactly one value arg.
///
/// # Safety
/// Avoid the following undefined behavior:
/// - Passing the wrong argument type as the config value (each key documents the expected value).
#[no_mangle]
pub unsafe extern "C" fn sealevel_config_setopt(
    config: *mut sealevel_config,
    key: sealevel_config_opt,
    value: usize,
) {
    match key {
        SEALEVEL_OPT_NONE => (),
        SEALEVEL_OPT_NO_VERIFY => (*config).no_verify = value != 0,
        SEALEVEL_OPT_MAX_CALL_DEPTH => (*config).config.max_call_depth = value,
        SEALEVEL_OPT_STACK_FRAME_SIZE => (*config).config.stack_frame_size = value,
        SEALEVEL_OPT_ENABLE_STACK_FRAME_GAPS => {
            (*config).config.enable_stack_frame_gaps = value != 0
        }
        SEALEVEL_OPT_INSTRUCTION_METER_CHECKPOINT_DISTANCE => {
            (*config).config.instruction_meter_checkpoint_distance = value
        }
        SEALEVEL_OPT_ENABLE_INSTRUCTION_METER => {
            (*config).config.enable_instruction_meter = value != 0
        }
        SEALEVEL_OPT_ENABLE_INSTRUCTION_TRACING => {
            (*config).config.enable_instruction_tracing = value != 0
        }
        SEALEVEL_OPT_ENABLE_SYMBOL_AND_SECTION_LABELS => {
            (*config).config.enable_symbol_and_section_labels = value != 0
        }
        SEALEVEL_OPT_DISABLE_UNRESOLVED_SYMBOLS_AT_RUNTIME => {
            (*config).config.disable_unresolved_symbols_at_runtime = value != 0
        }
        SEALEVEL_OPT_REJECT_BROKEN_ELFS => (*config).config.reject_broken_elfs = value != 0,
        SEALEVEL_OPT_NOOP_INSTRUCTION_RATIO => {
            (*config).config.noop_instruction_rate = value as u32
        }
        SEALEVEL_OPT_SANITIZE_USER_PROVIDED_VALUES => {
            (*config).config.sanitize_user_provided_values = value != 0
        }
        SEALEVEL_OPT_ENCRYPT_ENVIRONMENT_REGISTERS => {
            (*config).config.encrypt_environment_registers = value != 0
        }
        SEALEVEL_OPT_DISABLE_DEPRECATED_LOAD_INSTRUCTIONS => {
            (*config).config.disable_deprecated_load_instructions = value != 0
        }
        SEALEVEL_OPT_SYSCALL_BPF_FUNCTION_HASH_COLLISION => {
            (*config).config.syscall_bpf_function_hash_collision = value != 0
        }
        SEALEVEL_OPT_REJECT_CALLX_R10 => (*config).config.reject_callx_r10 = value != 0,
        SEALEVEL_OPT_DYNAMIC_STACK_FRAMES => (*config).config.dynamic_stack_frames = value != 0,
        SEALEVEL_OPT_ENABLE_SDIV => (*config).config.enable_sdiv = value != 0,
        SEALEVEL_OPT_OPTIMIZE_RODATA => (*config).config.optimize_rodata = value != 0,
        SEALEVEL_OPT_STATIC_SYSCALLS => (*config).config.static_syscalls = value != 0,
        SEALEVEL_OPT_ENABLE_ELF_VADDR => (*config).config.enable_elf_vaddr = value != 0,
    }
}

/// Releases resources associated with a Sealevel machine config.
///
/// # Safety
/// Avoid the following undefined behavior:
/// - Calling this function given a string that's _not_ the return value of `sealevel_config_new`.
/// - Calling this function more than once on the same object (double free).
/// - Using the config object after calling this function (use-after-free).
#[no_mangle]
pub unsafe extern "C" fn sealevel_config_free(config: *mut sealevel_config) {
    drop(Box::from_raw(config))
}
