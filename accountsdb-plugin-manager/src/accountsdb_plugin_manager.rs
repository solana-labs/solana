/// Managing the AccountsDb plugins
use {
    libloading::{Library, Symbol},
    log::*,
    solana_accountsdb_plugin_intf::accountsdb_plugin_intf::AccountsDbPlugin,
    std::error::Error,
};

pub struct AccountsDbPluginManager {
    plugins: Vec<Box<dyn AccountsDbPlugin>>,
    libs: Vec<Library>,
}

impl AccountsDbPluginManager {
    pub fn new() -> Self {
        AccountsDbPluginManager {
            plugins: Vec::default(),
            libs: Vec::default(),
        }
    }

    pub unsafe fn load_plugin(&mut self, libpath: &str) -> Result<(), Box<dyn Error>> {
        type PluginConstructor = unsafe fn() -> *mut dyn AccountsDbPlugin;
        let lib = Library::new(libpath)?;
        let constructor: Symbol<PluginConstructor> = lib.get(b"_create_plugin")?;
        let plugin_raw = constructor();
        let plugin = Box::from_raw(plugin_raw);
        plugin.on_load();
        self.plugins.push(plugin);
        self.libs.push(lib);
        Ok(())
    }

    /// Unload all plugins and loaded plugin libraries, making sure to fire
    /// their `on_plugin_unload()` methods so they can do any necessary cleanup.
    pub fn unload(&mut self) {
        for plugin in self.plugins.drain(..) {
            info!("Unloading plugin for {:?}", plugin.name());
            plugin.on_unload();
        }

        for lib in self.libs.drain(..) {
            drop(lib);
        }
    }
}
