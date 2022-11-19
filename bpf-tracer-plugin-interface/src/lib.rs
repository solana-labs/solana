use crate::bpf_tracer_plugin_interface::BpfTracerPlugin;
use std::fmt::Debug;

pub mod bpf_tracer_plugin_interface;

pub trait BpfTracerPluginManager: Debug + Send + Sync {
    fn bpf_tracer_plugins(&self) -> &[Box<dyn BpfTracerPlugin>];
    fn bpf_tracer_plugins_mut(&mut self) -> &mut [Box<dyn BpfTracerPlugin>];
}
