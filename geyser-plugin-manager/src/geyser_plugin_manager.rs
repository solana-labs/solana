use {
    jsonrpc_core::{ErrorCode, Result as JsonRpcResult},
    jsonrpc_server_utils::tokio::sync::oneshot::Sender as OneShotSender,
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
        Ok(())
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

    pub fn list_plugins_rpc(&self) -> JsonRpcResult<Vec<String>> {
        Ok(self.plugins.iter().map(|p| p.name().to_owned()).collect())
    }

    pub fn load_plugin_rpc(&mut self, libpath: &str, config_file: &str) -> JsonRpcResult<String> {
        // First load plugin
        let mut new_plugin: Box<dyn GeyserPlugin> = unsafe { load_plugin(libpath)? };

        // Then see if a plugin with this name already exists. If so, abort
        if self
            .plugins
            .iter()
            .any(|plugin| plugin.name().eq(new_plugin.name()))
        {
            return Err(jsonrpc_core::Error {
                code: ErrorCode::InternalError,
                message: format!(
                    "There already exists a plugin {} loaded. Did not load requested plugin",
                    new_plugin.name()
                ),
                data: None,
            });
        }

        // Call on_load and push plugin
        new_plugin
            .on_load(config_file)
            .map_err(|on_load_err| jsonrpc_core::Error {
                code: ErrorCode::InternalError,
                message: format!("on_load method of plugin failed: {on_load_err}",),
                data: None,
            })?;
        let name = new_plugin.name().to_string();
        self.plugins.push(new_plugin);

        Ok(name)
    }

    pub fn unload_plugin_rpc(&mut self, name: &str) -> JsonRpcResult<()> {
        // Check if any plugin names match this one
        let Some(idx) = self.plugins.iter().position(|plugin| plugin.name().eq(name)) else {
            // If we don't find one return an error
            return Err(
                jsonrpc_core::error::Error {
                    code: ErrorCode::InternalError,
                    message: String::from("plugin requested to unload is not loaded"),
                    data: None,
                }
            )
        };

        // Unload and drop plugin
        let mut current_plugin = self.plugins.remove(idx);
        current_plugin.on_unload();
        drop(current_plugin);

        Ok(())
    }

    /// Checks for a plugin with a given `name`.
    /// If it exists, first unload it.
    /// Then, attempt to load a new plugin
    pub fn reload_plugin_rpc(
        &mut self,
        name: &str,
        #[cfg_attr(test, allow(unused_variables))] libpath: &str,
        config_file: &str,
    ) -> JsonRpcResult<()> {
        // Check if any plugin names match this one
        let Some(idx) = self.plugins.iter().position(|plugin| plugin.name().eq(name)) else {
            // If we don't find one return an error
            return Err(
                jsonrpc_core::error::Error {
                    code: ErrorCode::InternalError,
                    message: String::from("plugin requested to reload is not loaded"),
                    data: None,
                }
            )
        };

        // Unload and drop current plugin first in case plugin requires exclusive access to resource,
        // such as a particular port or database.
        let mut current_plugin = self.plugins.remove(idx);
        current_plugin.on_unload();
        drop(current_plugin);

        // Try to load plugin, library
        // SAFETY: It is up to the validator to ensure this is a valid plugin library.
        let mut new_plugin: Box<dyn GeyserPlugin> = unsafe { load_plugin(libpath)? };

        // Attempt to on_load with new plugin
        match new_plugin.on_load(&config_file) {
            // On success, replace plugin and library
            Ok(()) => {
                self.plugins.push(new_plugin);
            }

            // On success, replace plugin and library
            Err(err) => {
                return Err(jsonrpc_core::error::Error {
                    code: ErrorCode::InternalError,
                    message: format!(
                        "failed to start new plugin (previous plugin was dropped!): {err}"
                    ),
                    data: None,
                });
            }
        }

        Ok(())
    }
}

/// This function is used both in the load and reload admin rpc calls
#[inline(always)]
unsafe fn load_plugin(libpath: &str) -> JsonRpcResult<Box<dyn GeyserPlugin>> {
    // This is mocked for tests to avoid having to do IO with a dynamically linked library
    // across different architectures at test time
    #[cfg(test)]
    {
        Ok(tests::dummy_plugin())
    }

    #[cfg(not(test))]
    {
        // Attempt to load Library
        let lib = Library::new(libpath).map_err(|e| jsonrpc_core::error::Error {
            code: ErrorCode::InternalError,
            message: format!("failed to load geyser plugin library: {e}"),
            data: None,
        })?;

        // Attempt to retrieve GeyserPlugin constructor
        let constructor: Symbol<PluginConstructor> =
            lib.get(b"_create_plugin")
                .map_err(|e| jsonrpc_core::error::Error {
                    code: ErrorCode::InternalError,
                    message: format!(
                        "failed to get plugin constructor _create_plugin from library: {e}"
                    ),
                    data: None,
                })?;

        // Attempt to construct raw *mut dyn GeyserPlugin
        // This may fail with bad plugin constructor, with e.g. unwraps.
        // Let's catch panic and propagate error instead of crashing entire validator
        let plugin_raw =
            std::panic::catch_unwind(|| constructor()).map_err(|e| jsonrpc_core::error::Error {
                code: ErrorCode::InternalError,
                message: format!("failed to start new plugin, previous plugin was dropped: {e:?}"),
                data: None,
            })?;

        Ok(Box::from_raw(plugin_raw))
    }
}

#[derive(Debug)]
pub enum PluginManagerRequest {
    ReloadPlugin {
        name: String,
        libpath: String,
        config_file: String,
        tx: OneShotSender<JsonRpcResult<()>>,
    },
    UnloadPlugin {
        name: String,
        tx: OneShotSender<JsonRpcResult<()>>,
    },
    LoadPlugin {
        libpath: String,
        config_file: String,
        tx: OneShotSender<JsonRpcResult<String>>,
    },
    ListPlugins {
        tx: OneShotSender<JsonRpcResult<Vec<String>>>,
    },
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, RwLock};

    use crate::geyser_plugin_manager::GeyserPluginManager;
    use libloading::Library;
    use solana_geyser_plugin_interface::geyser_plugin_interface::GeyserPlugin;

    #[allow(unused)]
    /// This is here in case more tests are written which need a dummy library
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

    #[allow(unused)]
    /// This is here in case more tests are written which need a dummy library
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

    pub(super) fn dummy_plugin() -> Box<dyn GeyserPlugin> {
        let plugin = Box::new(TestPlugin);
        plugin
    }

    pub(super) fn dummy_plugin2() -> Box<dyn GeyserPlugin> {
        let plugin = Box::new(TestPlugin2);
        plugin
    }

    pub(super) fn dummy_plugin3() -> Box<dyn GeyserPlugin> {
        let plugin = Box::new(TestPlugin3);
        plugin
    }

    const DUMMY_NAME: &'static str = "dummy";
    const DUMMY_CONFIG_FILE: &'static str = "dummy_config";
    const DUMMY_LIBRARY: &'static str = "dummy_lib";
    const ANOTHER_DUMMY_NAME: &'static str = "another_dummy";
    const YET_ANOTHER_DUMMY_NAME: &'static str = "another_dummy";

    #[derive(Debug)]
    pub(super) struct TestPlugin;

    impl GeyserPlugin for TestPlugin {
        fn name(&self) -> &'static str {
            DUMMY_NAME
        }
    }

    #[derive(Debug)]
    pub(super) struct TestPlugin2;

    impl GeyserPlugin for TestPlugin2 {
        fn name(&self) -> &'static str {
            ANOTHER_DUMMY_NAME
        }
    }

    #[derive(Debug)]
    pub(super) struct TestPlugin3;

    impl GeyserPlugin for TestPlugin3 {
        fn name(&self) -> &'static str {
            YET_ANOTHER_DUMMY_NAME
        }
    }

    #[test]
    fn test_geyser_reload() {
        // Initialize empty manager
        let plugin_manager = Arc::new(RwLock::new(GeyserPluginManager::new()));

        // No plugins are loaded, this should fail
        let mut plugin_manager_lock = plugin_manager.write().unwrap();
        let reload_result =
            plugin_manager_lock.reload_plugin_rpc(DUMMY_NAME, DUMMY_LIBRARY, DUMMY_CONFIG_FILE);
        assert_eq!(
            reload_result.unwrap_err().message,
            "plugin requested to reload is not loaded"
        );

        // Mock having loaded plugin (TestPlugin)
        let mut plugin = dummy_plugin();
        plugin.on_load("").unwrap();
        plugin_manager_lock.plugins.push(plugin);
        // plugin_manager_lock.libs.push(lib);
        assert_eq!(plugin_manager_lock.plugins[0].name(), DUMMY_NAME);
        plugin_manager_lock.plugins[0].name();

        // Try wrong name (same error)
        const WRONG_NAME: &'static str = "wrong_name";
        let reload_result =
            plugin_manager_lock.reload_plugin_rpc(WRONG_NAME, DUMMY_LIBRARY, DUMMY_CONFIG_FILE);
        assert_eq!(
            reload_result.unwrap_err().message,
            "plugin requested to reload is not loaded"
        );

        // Now try a (dummy) reload, replacing TestPlugin with TestPlugin2
        let reload_result =
            plugin_manager_lock.reload_plugin_rpc(DUMMY_NAME, DUMMY_LIBRARY, DUMMY_CONFIG_FILE);
        assert!(reload_result.is_ok());
    }

    #[test]
    fn test_plugin_list() {
        // Initialize empty manager
        let plugin_manager = Arc::new(RwLock::new(GeyserPluginManager::new()));
        let mut plugin_manager_lock = plugin_manager.write().unwrap();

        // Load two plugins
        // First
        let mut plugin = dummy_plugin();
        plugin.on_load(DUMMY_CONFIG_FILE).unwrap();
        plugin_manager_lock.plugins.push(plugin);
        // Second
        let mut plugin = dummy_plugin3();
        plugin.on_load(DUMMY_CONFIG_FILE).unwrap();
        plugin_manager_lock.plugins.push(plugin);

        // Check that both plugins are returned in the list
        let plugins = plugin_manager_lock.list_plugins_rpc().unwrap();
        assert!(plugins.iter().any(|name| name.eq(DUMMY_NAME)));
        assert!(plugins.iter().any(|name| name.eq(YET_ANOTHER_DUMMY_NAME)));
    }

    #[test]
    fn test_plugin_load_unload() {
        // Initialize empty manager
        let plugin_manager = Arc::new(RwLock::new(GeyserPluginManager::new()));
        let mut plugin_manager_lock = plugin_manager.write().unwrap();

        // Load rpc call
        let load_result = plugin_manager_lock.load_plugin_rpc(DUMMY_LIBRARY, DUMMY_CONFIG_FILE);
        assert!(load_result.is_ok());
        assert_eq!(plugin_manager_lock.plugins.len(), 1);

        // Unload rpc call
        let unload_result = plugin_manager_lock.unload_plugin_rpc(DUMMY_NAME);
        assert!(unload_result.is_ok());
        assert_eq!(plugin_manager_lock.plugins.len(), 0);
    }
}
