use crate::bpf_tracer_plugin_interface::BpfTracerPlugin;
use std::fmt::Debug;
use std::sync::{Arc, RwLock};

pub mod bpf_tracer_plugin_interface;

pub trait BpfTracerPluginManager: Debug + Send + Sync {
    fn bpf_tracer_plugins(&self) -> Box<dyn Iterator<Item = &Box<dyn BpfTracerPlugin>> + '_>;

    fn active_bpf_tracer_plugins_mut(
        &mut self,
    ) -> Box<dyn Iterator<Item = &mut Box<dyn BpfTracerPlugin>> + '_>;
}

pub fn has_bpf_tracing_plugins(
    bpf_tracer_plugin_manager: &Option<Arc<RwLock<dyn BpfTracerPluginManager>>>,
) -> bool {
    bpf_tracer_plugin_manager
        .as_ref()
        .map(|plugins| {
            plugins
                .read()
                .unwrap()
                .bpf_tracer_plugins()
                .next()
                .is_some()
        })
        .unwrap_or(false)
}
