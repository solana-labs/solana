use solana_rbpf::vm::Config;

/// A virtual machine capable of executing Solana Sealevel programs.
#[derive(Default)]
pub struct sealevel_machine {
    pub(crate) config: Config,
}

impl sealevel_machine {
    fn new() -> Self {
        Self::default()
    }
}

/// Creates a new Sealevel machine environment.
#[no_mangle]
pub unsafe extern "C" fn sealevel_machine_new() -> *mut sealevel_machine {
    let machine = sealevel_machine::new();
    Box::into_raw(Box::new(machine))
}

/// Releases resources associated with a Sealevel machine.
#[no_mangle]
pub unsafe extern "C" fn sealevel_machine_free(machine: *mut sealevel_machine) {
    drop(Box::from_raw(machine))
}
