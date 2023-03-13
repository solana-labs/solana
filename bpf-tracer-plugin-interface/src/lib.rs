use crate::bpf_tracer_plugin_interface::BpfTracerPlugin;
use std::fmt::Debug;
use std::sync::{Arc, RwLock};

pub mod bpf_tracer_plugin_interface;

pub trait BpfTracerPluginManager: Debug + Send + Sync {
    fn bpf_tracer_plugins(&self) -> &[Box<dyn BpfTracerPlugin>];
    fn bpf_tracer_plugins_mut(&mut self) -> &mut [Box<dyn BpfTracerPlugin>];
}

pub fn has_bpf_tracing_plugins(
    bpf_tracer_plugin_manager: &Option<Arc<RwLock<dyn BpfTracerPluginManager>>>,
) -> bool {
    bpf_tracer_plugin_manager
        .as_ref()
        .map(|plugins| !plugins.read().unwrap().bpf_tracer_plugins().is_empty())
        .unwrap_or(false)
}
