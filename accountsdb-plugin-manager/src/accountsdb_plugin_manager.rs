/// Managing the AccountsDb plugins
use {
    libloading::{Library, Symbol},
    log::*,
    solana_accountsdb_plugin_interface::accountsdb_plugin_interface::AccountsDbPlugin,
    std::error::Error,
};

#[derive(Default, Debug)]
pub struct AccountsDbPluginManager {
    pub plugins: Vec<Box<dyn AccountsDbPlugin>>,
    libs: Vec<Library>,
}

impl AccountsDbPluginManager {
    pub fn new() -> Self {
        AccountsDbPluginManager {
            plugins: Vec::default(),
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
        type PluginConstructor = unsafe fn() -> *mut dyn AccountsDbPlugin;
        let lib = Library::new(libpath)?;
        let constructor: Symbol<PluginConstructor> = lib.get(b"_create_plugin")?;
        let plugin_raw = constructor();
        let mut plugin = Box::from_raw(plugin_raw);
        plugin.on_load(config_file)?;
        self.plugins.push(plugin);
        self.libs.push(lib);
        Ok(())
    }

    /// Unload all plugins and loaded plugin libraries, making sure to fire
    /// their `on_plugin_unload()` methods so they can do any necessary cleanup.
    pub fn unload(&mut self) {
        for mut plugin in self.plugins.drain(..) {
            info!("Unloading plugin for {:?}", plugin.name());
            plugin.on_unload();
        }

        for lib in self.libs.drain(..) {
            drop(lib);
        }
    }
}
