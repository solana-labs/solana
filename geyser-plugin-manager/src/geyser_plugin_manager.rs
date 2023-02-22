use std::{path::PathBuf, sync::RwLockWriteGuard};

/// Managing the Geyser plugins
use {
    libloading::{Library, Symbol},
    log::*,
    solana_geyser_plugin_interface::geyser_plugin_interface::GeyserPlugin,
    std::error::Error,
};

#[derive(Default, Debug)]
pub struct GeyserPluginManager {
    pub plugins: Vec<Box<dyn GeyserPlugin>>,
    pub libs: Vec<Library>,
    pub libpaths: Vec<PathBuf>,
}

type PluginConstructor = unsafe fn() -> *mut dyn GeyserPlugin;

impl GeyserPluginManager {
    pub fn new() -> Self {
        GeyserPluginManager {
            plugins: Vec::default(),
            libs: Vec::default(),
            libpaths: Vec::default(),
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
        let constructor: Symbol<PluginConstructor> = lib.get(b"_create_plugin")?;
        let plugin_raw = constructor();
        let mut plugin = Box::from_raw(plugin_raw);
        plugin.on_load(config_file)?;
        self.plugins.push(plugin);
        self.libs.push(lib);
        self.libpaths.push(libpath.into());
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

    // Get a mutable reference to a particular plugin and library
    fn get_plugin_and_lib_mut(
        &mut self,
        index: usize,
    ) -> Option<(&mut Box<dyn GeyserPlugin>, &mut Library)> {
        // Lengths should always be the same, so only need to check one
        if index < self.plugins.len() {
            Some((
                self.plugins.get_mut(index).unwrap(),
                self.libs.get_mut(index).unwrap(),
            ))
        } else {
            None
        }
    }

    pub fn reload_plugin(
        mut plugin_manager: RwLockWriteGuard<GeyserPluginManager>,
        name: &str,
        libpath: &str,
        config_file: &str,
    ) -> Result<(), PluginManagerError> {
        // Check if any plugin names match this one
        let Some(idx) = plugin_manager.plugins.iter().position(|plugin| plugin.name().eq(name)) else {
            // If we don't find one, drop write lock ASAP and return an error
            drop(plugin_manager);
            return Err(PluginManagerError::PluginNotFoundDuringReload)
        };

        // Get current plugin and library
        let (current_plugin, current_lib) = plugin_manager
            .get_plugin_and_lib_mut(idx)
            .expect("just checked for existence of libpath");

        // Unload first in case plugin requires exclusive access to resource,
        // such as a particular port or database.
        current_plugin.on_unload();

        // Try to load plugin, library
        // SAFETY: It is up the validator to ensure this is a valid plugin library.
        let (mut new_plugin, new_lib): (Box<dyn GeyserPlugin>, Library) = {
            #[cfg(not(test))]
            unsafe {
                // Attempt to load Library
                let lib = Library::new(libpath).map_err(|e| {
                    PluginManagerError::FailedToLoadNewLibrary {
                        err: format!("{e}"),
                    }
                })?;

                // Attempt to retrieve GeyserPlugin constructor
                let constructor: Symbol<PluginConstructor> =
                    lib.get(b"_create_plugin").map_err(|e| {
                        PluginManagerError::FailedToGetConstructorFromLibrary {
                            err: format!("{e}"),
                        }
                    })?;

                // Attempt to construct raw *mut dyn GeyserPlugin
                let plugin_raw = constructor();

                (Box::from_raw(plugin_raw), lib)
            }

            #[cfg(test)]
            {
                // This is mocked for tests to avoid having to do IO with a dynamically linked library
                // across different architectures at test time
                tests::dummy_plugin_and_library()
            }
        };

        // Try unload, on_load
        // Attempt to load new plugin
        match new_plugin.on_load(&config_file) {
            // On success, replace plugin and library
            // Note: don't need to replace libpath since it matches
            Ok(()) => {
                *current_plugin = new_plugin;
                *current_lib = new_lib;
            }

            // On failure, attempt to revert and return error
            // Note that here we are using the same config file as for the new file
            Err(err) => {
                match current_plugin.on_load(&config_file) {
                    // Failed to load plugin but successfully reverted
                    Ok(()) => {
                        return Err(PluginManagerError::FailedReloadRevertedToOldPlugin {
                            err: format!("{err}"),
                        })
                    }

                    // Failed to load plugin and failed to revert.
                    //
                    // Note that many plugin impls don't do anything fallible
                    // for on_load or on_unload so this should not happen very often
                    Err(revert_err) => {
                        // If we failed to revert, unload plugin
                        // First drop mutable references
                        drop(current_plugin);
                        drop(current_lib);
                        // Then drop plugin, lib, and path
                        drop(plugin_manager.plugins.remove(idx));
                        drop(plugin_manager.libs.remove(idx));
                        drop(plugin_manager.libpaths.remove(idx));

                        return Err(PluginManagerError::FailedReloadFailedReverted {
                            err: err.to_string(),
                            revert_err: revert_err.to_string(),
                        });
                    }
                };
            }
        }

        Ok(())
    }
}

// Encapsulates all possible errors that can occurr when dynamically managing plugins.
//
// Note: This is here:
// 1) to avoid adding jsoncore_rpc as a dependency to this crate
// 2) in case these these methods are called outside of an rpc context in the future.
#[derive(Debug, PartialEq)]
pub enum PluginManagerError {
    PluginNotFoundDuringReload,
    FailedToLoadNewLibrary { err: String },
    FailedToGetConstructorFromLibrary { err: String },
    FailedReloadRevertedToOldPlugin { err: String },
    FailedReloadFailedReverted { err: String, revert_err: String },
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, RwLock};

    use crate::geyser_plugin_manager::{GeyserPluginManager, PluginManagerError};
    use libloading::Library;
    use solana_geyser_plugin_interface::geyser_plugin_interface::GeyserPlugin;

    pub(super) fn dummy_plugin_and_library() -> (Box<dyn GeyserPlugin>, Library) {
        let plugin = Box::new(TestPlugin);
        let lib = {
            let handle: *mut std::os::raw::c_void = &mut () as *mut _ as *mut std::os::raw::c_void;
            // SAFETY: all calls to get Symbols should fail, so this is actually safe
            let inner_lib = unsafe { libloading::os::unix::Library::from_raw(handle) };
            Library::from(inner_lib)
        };
        (plugin, lib)
    }

    pub(super) fn dummy_plugin_and_library2() -> (Box<dyn GeyserPlugin>, Library) {
        let plugin = Box::new(TestPlugin2);
        let lib = {
            let handle: *mut std::os::raw::c_void = &mut () as *mut _ as *mut std::os::raw::c_void;
            // SAFETY: all calls to get Symbols should fail, so this is actually safe
            let inner_lib = unsafe { libloading::os::unix::Library::from_raw(handle) };
            Library::from(inner_lib)
        };
        (plugin, lib)
    }

    const DUMMY_NAME: &'static str = "dummy";

    #[derive(Debug)]
    pub(super) struct TestPlugin;

    // A global variable which helps track which plugin is loaded
    static TEST_UTIL: RwLock<Option<&'static str>> = RwLock::new(None);
    static PLUGIN_1_LOADED: &'static str = "plugin 1 loaded";
    static PLUGIN_2_LOADED: &'static str = "plugin 2 loaded";

    impl GeyserPlugin for TestPlugin {
        fn name(&self) -> &'static str {
            DUMMY_NAME
        }
        fn on_load(
            &mut self,
            _config_file: &str,
        ) -> solana_geyser_plugin_interface::geyser_plugin_interface::Result<()> {
            // Update global tracker
            *TEST_UTIL.write().unwrap() = Some(PLUGIN_1_LOADED);
            Ok(())
        }
    }

    #[derive(Debug)]
    pub(super) struct TestPlugin2;

    impl GeyserPlugin for TestPlugin2 {
        fn name(&self) -> &'static str {
            DUMMY_NAME
        }
        fn on_load(
            &mut self,
            _config_file: &str,
        ) -> solana_geyser_plugin_interface::geyser_plugin_interface::Result<()> {
            // Update global tracker
            *TEST_UTIL.write().unwrap() = Some(PLUGIN_2_LOADED);
            Ok(())
        }
    }

    #[test]
    fn test_geyser_reload() {
        // Initialize empty manager
        let plugin_manager = Arc::new(RwLock::new(GeyserPluginManager::new()));

        // The geyser plugin which we will reload
        const DUMMY_CONFIG_FILE: &'static str = "dummy_config";
        const DUMMY_LIBRARY: &'static str = "dummy_lib";

        // No plugins are loaded, this should fail
        let plugin_manager_lock = plugin_manager.write().unwrap();
        let reload_result = GeyserPluginManager::reload_plugin(
            plugin_manager_lock,
            DUMMY_NAME,
            DUMMY_LIBRARY,
            DUMMY_CONFIG_FILE,
        );
        assert_eq!(
            reload_result,
            Err(PluginManagerError::PluginNotFoundDuringReload)
        );

        // Mock having loaded plugin (TestPlugin2)
        let mut plugin_manager_lock = plugin_manager.write().unwrap();
        let (mut plugin, lib) = dummy_plugin_and_library2();
        plugin.on_load("");
        assert_eq!(*TEST_UTIL.read().unwrap(), Some(PLUGIN_2_LOADED));
        plugin_manager_lock.plugins.push(plugin);
        plugin_manager_lock.libs.push(lib);
        plugin_manager_lock.libpaths.push(DUMMY_LIBRARY.into());
        // This is a GeyserPlugin trait method
        assert_eq!(plugin_manager_lock.plugins[0].name(), DUMMY_NAME);
        plugin_manager_lock.plugins[0].name();
        drop(plugin_manager_lock);

        // Try wrong name (same error)
        const WRONG_NAME: &'static str = "wrong_name";
        let mut plugin_manager_lock = plugin_manager.write().unwrap();
        let reload_result = GeyserPluginManager::reload_plugin(
            plugin_manager_lock,
            WRONG_NAME,
            DUMMY_LIBRARY,
            DUMMY_CONFIG_FILE,
        );
        assert_eq!(
            reload_result,
            Err(PluginManagerError::PluginNotFoundDuringReload)
        );

        // Now try a (dummy) reload, replacing TestPlugin2 with TestPlugin
        let plugin_manager_lock = plugin_manager.write().unwrap();
        let reload_result = GeyserPluginManager::reload_plugin(
            plugin_manager_lock,
            DUMMY_NAME,
            DUMMY_LIBRARY,
            DUMMY_CONFIG_FILE,
        );
        assert_eq!(*TEST_UTIL.read().unwrap(), Some(PLUGIN_1_LOADED));
        assert!(reload_result.is_ok());
    }
}
