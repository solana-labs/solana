/// Managing the Geyser plugins
use {
    libloading::{Library, Symbol},
    log::*,
    solana_bpf_tracer_plugin_interface::{
        bpf_tracer_plugin_interface::BpfTracerPlugin, BpfTracerPluginManager,
    },
    solana_geyser_plugin_interface::geyser_plugin_interface::GeyserPlugin,
    std::error::Error,
};

#[derive(Default, Debug)]
pub struct GeyserPluginManager {
    pub geyser_plugins: Vec<Box<dyn GeyserPlugin>>,
    pub bpf_tracer_plugins: Vec<Box<dyn BpfTracerPlugin>>,
    libs: Vec<Library>,
}

impl GeyserPluginManager {
    pub fn new() -> Self {
        GeyserPluginManager {
            geyser_plugins: Vec::default(),
            bpf_tracer_plugins: Vec::default(),
            libs: Vec::default(),
        }
    }

    /// # Safety
    ///
    /// This function loads the dynamically linked library specified in the path. The library
    /// must do necessary initializations.
    pub unsafe fn load_plugin(
        &mut self,
        libpath: &str,
        config_file: &str,
    ) -> Result<(), Box<dyn Error>> {
        let lib = Library::new(libpath)?;

        let mut loaded = false;
        let mut resolving_errors = vec![];
        for symbol in ["_create_geyser_plugin", "_create_plugin"] {
            match Self::get_plugin::<dyn GeyserPlugin>(&lib, symbol) {
                Ok(mut plugin) => {
                    info!(
                        "Loading Geyser plugin for {:?} from {:?}",
                        plugin.name(),
                        libpath
                    );
                    plugin.on_load(config_file)?;
                    self.geyser_plugins.push(plugin);
                    loaded = true;
                }
                Err(error) => resolving_errors.push(error),
            }
        }

        match Self::get_plugin::<dyn BpfTracerPlugin>(&lib, "_create_bpf_tracer_plugin") {
            Ok(mut plugin) => {
                info!(
                    "Loading BPF Tracer plugin for {:?} from {:?}",
                    plugin.name(),
                    libpath
                );
                plugin.on_load(config_file)?;
                self.bpf_tracer_plugins.push(plugin);
                loaded = true;
            }
            Err(error) => resolving_errors.push(error),
        }

        if !loaded {
            error!("Failed to load plugins from the library: {:?}", libpath);
            return Err(resolving_errors
                .into_iter()
                .map(|err| err.to_string())
                .collect::<Vec<_>>()
                .join("; ")
                .into());
        }

        self.libs.push(lib);
        Ok(())
    }

    unsafe fn get_plugin<T: ?Sized>(
        lib: &Library,
        symbol: &'static str,
    ) -> Result<Box<T>, Box<dyn Error>> {
        type PluginConstructor<T> = unsafe fn() -> *mut T;
        let constructor: Symbol<PluginConstructor<T>> = lib.get(symbol.as_bytes())?;
        let plugin_raw = constructor();
        let plugin = Box::from_raw(plugin_raw);
        Ok(plugin)
    }

    /// Unload all plugins and loaded plugin libraries, making sure to fire
    /// their `on_plugin_unload()` methods so they can do any necessary cleanup.
    pub fn unload(&mut self) {
        for mut plugin in self.geyser_plugins.drain(..) {
            info!("Unloading Geyser plugin for {:?}", plugin.name());
            plugin.on_unload();
        }

        for mut plugin in self.bpf_tracer_plugins.drain(..) {
            info!("Unloading BPF Tracer plugin for {:?}", plugin.name());
            plugin.on_unload();
        }

        for lib in self.libs.drain(..) {
            drop(lib);
        }
    }

    /// Check if there is any plugin interested in account data
    pub fn account_data_notifications_enabled(&self) -> bool {
        for plugin in &self.geyser_plugins {
            if plugin.account_data_notifications_enabled() {
                return true;
            }
        }
        false
    }

    /// Check if there is any plugin interested in transaction data
    pub fn transaction_notifications_enabled(&self) -> bool {
        for plugin in &self.geyser_plugins {
            if plugin.transaction_notifications_enabled() {
                return true;
            }
        }
        false
    }
}

impl BpfTracerPluginManager for GeyserPluginManager {
    fn bpf_tracer_plugins(&mut self) -> &mut [Box<dyn BpfTracerPlugin>] {
        &mut self.bpf_tracer_plugins
    }
}
