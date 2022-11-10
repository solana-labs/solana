use {
    solana_rbpf::vm::TraceRecord,
    solana_sdk::{hash::Hash, pubkey::Pubkey},
    std::{
        any::Any,
        {error, io},
    },
    thiserror::Error,
};

/// Errors returned by plugin calls
#[derive(Error, Debug)]
pub enum BpfTracerPluginError {
    /// Error opening the configuration file; for example, when the file
    /// is not found or when the validator process has no permission to read it.
    #[error("Error opening config file. Error detail: ({0}).")]
    ConfigFileOpenError(#[from] io::Error),

    /// Error in reading the content of the config file or the content
    /// is not in the expected format.
    #[error("Error reading config file. Error message: ({msg})")]
    ConfigFileReadError { msg: String },

    /// Any custom error defined by the plugin.
    #[error("Plugin-defined custom error. Error message: ({0})")]
    Custom(Box<dyn error::Error + Send + Sync>),
}

pub type Result<T> = std::result::Result<T, BpfTracerPluginError>;

pub trait BpfTracerPlugin: Any + Send + Sync + std::fmt::Debug {
    fn name(&self) -> &'static str;

    /// The callback called when a plugin is loaded by the system, used for doing
    /// whatever initialization is required by the plugin. The _config_file
    /// contains the name of the config file. The config must be in JSON format
    /// and include a field "libpath" indicating the full path name of the shared
    /// library implementing this interface.
    #[allow(unused_variables)]
    fn on_load(&mut self, config_file: &str) -> Result<()> {
        Ok(())
    }

    /// The callback called right before a plugin is unloaded by the system
    /// Used for doing cleanup before unload.
    fn on_unload(&mut self) {}

    /// Check if the plugin is accepting BPF tracing.
    fn bpf_tracing_enabled(&self) -> bool {
        true
    }

    /// Called when BPF tracing is ready in `tracer` structure.
    fn trace_bpf<'a>(
        &mut self,
        program_id: &Pubkey,
        blockhash: &Hash,
        trace: &[TraceRecord<'a>],
    ) -> Result<()>;
}
