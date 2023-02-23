use {
    libloading::{Library, Symbol},
    log::*,
    solana_geyser_plugin_interface::geyser_plugin_interface::GeyserPlugin,
    std::error::Error,
};

#[derive(Default, Debug)]
pub struct GeyserPluginManager {
    pub plugins: Vec<Box<dyn GeyserPlugin>>,
}

type PluginConstructor = unsafe fn() -> *mut dyn GeyserPlugin;

impl GeyserPluginManager {
    pub fn new() -> Self {
        GeyserPluginManager {
            plugins: Vec::default(),
        }
    }

    /// # Safety
    ///
    /// This function loads the dynamically linked library specified in the path. The library
    /// must do necessary initializations.
    pub unsafe fn load_plugin(
        libpath: &str,
        config_file: &str,
    ) -> Result<Box<dyn GeyserPlugin>, Box<dyn Error>> {
        let lib = Library::new(libpath)?;
        let constructor: Symbol<PluginConstructor> = lib.get(b"_create_plugin")?;
        let plugin_raw = constructor();
        let mut plugin = Box::from_raw(plugin_raw);
        plugin.on_load(config_file)?;
        Ok(plugin)
    }

    /// Unload all plugins and loaded plugin libraries, making sure to fire
    /// their `on_plugin_unload()` methods so they can do any necessary cleanup.
    pub fn unload(&mut self) {
        for mut plugin in self.plugins.drain(..) {
            info!("Unloading plugin for {:?}", plugin.name());
            plugin.on_unload();
        }
    }

    /// Check if there is any plugin interested in account data
    pub fn account_data_notifications_enabled(&self) -> bool {
        for plugin in &self.plugins {
            if plugin.account_data_notifications_enabled() {
                return true;
            }
        }
        false
    }

    /// Check if there is any plugin interested in transaction data
    pub fn transaction_notifications_enabled(&self) -> bool {
        for plugin in &self.plugins {
            if plugin.transaction_notifications_enabled() {
                return true;
            }
        }
        false
    }
}
