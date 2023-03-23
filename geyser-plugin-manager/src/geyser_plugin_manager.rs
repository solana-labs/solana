use {
    jsonrpc_core::{ErrorCode, Result as JsonRpcResult},
    jsonrpc_server_utils::tokio::sync::oneshot::Sender as OneShotSender,
    libloading::Library,
    log::*,
    solana_bpf_tracer_plugin_interface::{
        bpf_tracer_plugin_interface::BpfTracerPlugin, BpfTracerPluginManager,
    },
    solana_geyser_plugin_interface::geyser_plugin_interface::GeyserPlugin,
    std::{error::Error, path::Path},
};

#[derive(Debug)]
pub enum Plugin {
    Geyser(Box<dyn GeyserPlugin>),
    BpfTracer(Box<dyn BpfTracerPlugin>),
}

impl Plugin {
    pub fn name(&self) -> &'static str {
        match self {
            Plugin::Geyser(plugin) => plugin.name(),
            Plugin::BpfTracer(plugin) => plugin.name(),
        }
    }

    pub fn on_load(&mut self, config_file: &str) -> Result<(), Box<dyn Error>> {
        match self {
            Plugin::Geyser(plugin) => plugin.on_load(config_file)?,
            Plugin::BpfTracer(plugin) => plugin.on_load(config_file)?,
        }
        Ok(())
    }

    pub fn on_unload(&mut self) {
        match self {
            Plugin::Geyser(plugin) => plugin.on_unload(),
            Plugin::BpfTracer(plugin) => plugin.on_unload(),
        }
    }
}

#[derive(Default, Debug)]
pub struct GeyserPluginManager {
    pub plugins: Vec<Plugin>,
    libs: Vec<Library>,
}

impl GeyserPluginManager {
    pub fn new() -> Self {
        GeyserPluginManager {
            plugins: Vec::default(),
            libs: Vec::default(),
        }
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
        for plugin in self.geyser_plugins() {
            if plugin.account_data_notifications_enabled() {
                return true;
            }
        }
        false
    }

    /// Check if there is any plugin interested in transaction data
    pub fn transaction_notifications_enabled(&self) -> bool {
        for plugin in self.geyser_plugins() {
            if plugin.transaction_notifications_enabled() {
                return true;
            }
        }
        false
    }

    /// Admin RPC request handler
    pub(crate) fn list_plugins(&self) -> JsonRpcResult<Vec<String>> {
        Ok(self.plugins.iter().map(|p| p.name().to_owned()).collect())
    }

    /// Admin RPC request handler
    /// # Safety
    ///
    /// This function loads the dynamically linked library specified in the path. The library
    /// must do necessary initializations.
    ///
    /// The string returned is the name of the plugin loaded, which can only be accessed once
    /// the plugin has been loaded and calling the name method.
    pub(crate) fn load_plugin(
        &mut self,
        plugin_config_file: impl AsRef<Path>,
    ) -> JsonRpcResult<String> {
        // First load plugin
        let (mut new_plugin, new_lib, new_config_file) =
            load_plugin_from_config(plugin_config_file.as_ref()).map_err(|e| {
                jsonrpc_core::Error {
                    code: ErrorCode::InvalidRequest,
                    message: format!("Failed to load plugin: {e}"),
                    data: None,
                }
            })?;

        // Then see if a plugin with this name already exists. If so, abort
        if self
            .plugins
            .iter()
            .any(|plugin| plugin.name().eq(new_plugin.name()))
        {
            return Err(jsonrpc_core::Error {
                code: ErrorCode::InvalidRequest,
                message: format!(
                    "There already exists a plugin named {} loaded. Did not load requested plugin",
                    new_plugin.name()
                ),
                data: None,
            });
        }

        // Call on_load and push plugin
        new_plugin
            .on_load(new_config_file)
            .map_err(|on_load_err| jsonrpc_core::Error {
                code: ErrorCode::InvalidRequest,
                message: format!(
                    "on_load method of plugin {} failed: {on_load_err}",
                    new_plugin.name()
                ),
                data: None,
            })?;
        let name = new_plugin.name().to_string();
        self.plugins.push(new_plugin);
        self.libs.push(new_lib);

        Ok(name)
    }

    pub(crate) fn unload_plugin(&mut self, name: &str) -> JsonRpcResult<()> {
        // Check if any plugin names match this one
        let Some(idx) = self.plugins.iter().position(|plugin| plugin.name().eq(name)) else {
            // If we don't find one return an error
            return Err(
                jsonrpc_core::error::Error {
                    code: ErrorCode::InvalidRequest,
                    message: String::from("The plugin you requested to unload is not loaded"),
                    data: None,
                }
            )
        };

        // Unload and drop plugin and lib
        self._drop_plugin(idx);

        Ok(())
    }

    /// Checks for a plugin with a given `name`.
    /// If it exists, first unload it.
    /// Then, attempt to load a new plugin
    pub(crate) fn reload_plugin(&mut self, name: &str, config_file: &str) -> JsonRpcResult<()> {
        // Check if any plugin names match this one
        let Some(idx) = self.plugins.iter().position(|plugin| plugin.name().eq(name)) else {
            // If we don't find one return an error
            return Err(
                jsonrpc_core::error::Error {
                    code: ErrorCode::InvalidRequest,
                    message: String::from("The plugin you requested to reload is not loaded"),
                    data: None,
                }
            )
        };

        // Unload and drop current plugin first in case plugin requires exclusive access to resource,
        // such as a particular port or database.
        self._drop_plugin(idx);

        // Try to load plugin, library
        // SAFETY: It is up to the validator to ensure this is a valid plugin library.
        let (mut new_plugin, new_lib, new_parsed_config_file) =
            load_plugin_from_config(config_file.as_ref()).map_err(|err| jsonrpc_core::Error {
                code: ErrorCode::InvalidRequest,
                message: err.to_string(),
                data: None,
            })?;

        // Attempt to on_load with new plugin
        match new_plugin.on_load(new_parsed_config_file) {
            // On success, push plugin and library
            Ok(()) => {
                self.plugins.push(new_plugin);
                self.libs.push(new_lib);
            }

            // On failure, return error
            Err(err) => {
                return Err(jsonrpc_core::error::Error {
                    code: ErrorCode::InvalidRequest,
                    message: format!(
                        "Failed to start new plugin (previous plugin was dropped!): {err}"
                    ),
                    data: None,
                });
            }
        }

        Ok(())
    }

    pub fn geyser_plugins(&self) -> impl Iterator<Item = &Box<dyn GeyserPlugin>> {
        self.plugins.iter().filter_map(|plugin| match plugin {
            Plugin::Geyser(geyser_plugin) => Some(geyser_plugin),
            _ => None,
        })
    }

    fn _drop_plugin(&mut self, idx: usize) {
        let mut current_plugin = self.plugins.remove(idx);
        let _current_lib = self.libs.remove(idx);
        current_plugin.on_unload();
    }
}

#[derive(Debug)]
pub enum GeyserPluginManagerRequest {
    ReloadPlugin {
        name: String,
        config_file: String,
        response_sender: OneShotSender<JsonRpcResult<()>>,
    },
    UnloadPlugin {
        name: String,
        response_sender: OneShotSender<JsonRpcResult<()>>,
    },
    LoadPlugin {
        config_file: String,
        response_sender: OneShotSender<JsonRpcResult<String>>,
    },
    ListPlugins {
        response_sender: OneShotSender<JsonRpcResult<Vec<String>>>,
    },
}

#[derive(thiserror::Error, Debug)]
pub enum GeyserPluginManagerError {
    #[error("Cannot open the the plugin config file")]
    CannotOpenConfigFile(String),

    #[error("Cannot read the the plugin config file")]
    CannotReadConfigFile(String),

    #[error("The config file is not in a valid Json format")]
    InvalidConfigFileFormat(String),

    #[error("Plugin library path is not specified in the config file")]
    LibPathNotSet,

    #[error("Invalid plugin path")]
    InvalidPluginPath,

    #[error("Cannot load plugin shared library")]
    PluginLoadError(String),

    #[error("The geyser plugin {0} is already loaded shared library")]
    PluginAlreadyLoaded(String),

    #[error("The GeyserPlugin on_load method failed")]
    PluginStartError(String),
}

/// # Safety
///
/// This function loads the dynamically linked library specified in the path. The library
/// must do necessary initializations.
///
/// This returns the plugin, the dynamic library, and the parsed config file as a &str.
/// (The geyser or BPF tracer plugin interface requires a &str for the on_load method).
#[cfg(not(test))]
pub(crate) fn load_plugin_from_config(
    plugin_config_file: &Path,
) -> Result<(Plugin, Library, &str), GeyserPluginManagerError> {
    use std::{fs::File, io::Read, path::PathBuf};

    let mut file = match File::open(plugin_config_file) {
        Ok(file) => file,
        Err(err) => {
            return Err(GeyserPluginManagerError::CannotOpenConfigFile(format!(
                "Failed to open the plugin config file {plugin_config_file:?}, error: {err:?}"
            )));
        }
    };

    let mut contents = String::new();
    if let Err(err) = file.read_to_string(&mut contents) {
        return Err(GeyserPluginManagerError::CannotReadConfigFile(format!(
            "Failed to read the plugin config file {plugin_config_file:?}, error: {err:?}"
        )));
    }

    let result: serde_json::Value = match json5::from_str(&contents) {
        Ok(value) => value,
        Err(err) => {
            return Err(GeyserPluginManagerError::InvalidConfigFileFormat(format!(
                "The config file {plugin_config_file:?} is not in a valid Json5 format, error: {err:?}"
            )));
        }
    };

    let libpath = result["libpath"]
        .as_str()
        .ok_or(GeyserPluginManagerError::LibPathNotSet)?;
    let mut libpath = PathBuf::from(libpath);
    if libpath.is_relative() {
        let config_dir = plugin_config_file.parent().ok_or_else(|| {
            GeyserPluginManagerError::CannotOpenConfigFile(format!(
                "Failed to resolve parent of {plugin_config_file:?}",
            ))
        })?;
        libpath = config_dir.join(libpath);
    }

    let config_file = plugin_config_file
        .as_os_str()
        .to_str()
        .ok_or(GeyserPluginManagerError::InvalidPluginPath)?;

    let mut resolving_errors = vec![];
    unsafe {
        let lib = Library::new(libpath)
            .map_err(|e| GeyserPluginManagerError::PluginLoadError(e.to_string()))?;

        for symbol in ["_create_geyser_plugin", "_create_plugin"] {
            match get_plugin::<dyn GeyserPlugin>(&lib, symbol) {
                Ok(plugin) => return Ok((Plugin::Geyser(plugin), lib, config_file)),
                Err(error) => resolving_errors.push(error),
            }
        }

        match get_plugin::<dyn BpfTracerPlugin>(&lib, "_create_bpf_tracer_plugin") {
            Ok(plugin) => return Ok((Plugin::BpfTracer(plugin), lib, config_file)),
            Err(error) => resolving_errors.push(error),
        }
    }

    Err(GeyserPluginManagerError::PluginLoadError(
        resolving_errors
            .into_iter()
            .map(|err| err.to_string())
            .collect::<Vec<_>>()
            .join("; "),
    ))
}

/// # Safety
///
/// The function which name provided by `symbol` must return object with expected interface <T>.
#[cfg(not(test))]
unsafe fn get_plugin<T: ?Sized>(
    lib: &Library,
    symbol: &'static str,
) -> Result<Box<T>, libloading::Error> {
    type PluginConstructor<T> = unsafe fn() -> *mut T;
    let constructor: libloading::Symbol<PluginConstructor<T>> = lib.get(symbol.as_bytes())?;
    let plugin_raw = constructor();
    let plugin = Box::from_raw(plugin_raw);
    Ok(plugin)
}

// This is mocked for tests to avoid having to do IO with a dynamically linked library
// across different architectures at test time
//
/// This returns mocked values for the geyser plugin, the dynamic library, and the parsed config file as a &str.
/// (The geyser plugin interface requires a &str for the on_load method).
#[cfg(test)]
pub(crate) fn load_plugin_from_config(
    _geyser_plugin_config_file: &Path,
) -> Result<(Plugin, Library, &str), GeyserPluginManagerError> {
    Ok(tests::dummy_plugin_and_library())
}

#[cfg(test)]
mod tests {
    use crate::geyser_plugin_manager::Plugin;
    use {
        crate::geyser_plugin_manager::GeyserPluginManager,
        libloading::Library,
        solana_geyser_plugin_interface::geyser_plugin_interface::GeyserPlugin,
        std::sync::{Arc, RwLock},
    };

    pub(super) fn dummy_plugin_and_library() -> (super::Plugin, Library, &'static str) {
        let plugin = Box::new(TestPlugin);
        let lib = {
            let handle: *mut std::os::raw::c_void = &mut () as *mut _ as *mut std::os::raw::c_void;
            // SAFETY: all calls to get Symbols should fail, so this is actually safe
            let inner_lib = unsafe { libloading::os::unix::Library::from_raw(handle) };
            Library::from(inner_lib)
        };
        (Plugin::Geyser(plugin), lib, DUMMY_CONFIG)
    }

    pub(super) fn dummy_plugin_and_library2() -> (Plugin, Library, &'static str) {
        let plugin = Box::new(TestPlugin2);
        let lib = {
            let handle: *mut std::os::raw::c_void = &mut () as *mut _ as *mut std::os::raw::c_void;
            // SAFETY: all calls to get Symbols should fail, so this is actually safe
            let inner_lib = unsafe { libloading::os::unix::Library::from_raw(handle) };
            Library::from(inner_lib)
        };
        (Plugin::Geyser(plugin), lib, DUMMY_CONFIG)
    }

    const DUMMY_NAME: &str = "dummy";
    pub(super) const DUMMY_CONFIG: &str = "dummy_config";
    const ANOTHER_DUMMY_NAME: &str = "another_dummy";

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

    #[test]
    fn test_geyser_reload() {
        // Initialize empty manager
        let plugin_manager = Arc::new(RwLock::new(GeyserPluginManager::new()));

        // No plugins are loaded, this should fail
        let mut plugin_manager_lock = plugin_manager.write().unwrap();
        let reload_result = plugin_manager_lock.reload_plugin(DUMMY_NAME, DUMMY_CONFIG);
        assert_eq!(
            reload_result.unwrap_err().message,
            "The plugin you requested to reload is not loaded"
        );

        // Mock having loaded plugin (TestPlugin)
        let (mut plugin, lib, config) = dummy_plugin_and_library();
        plugin.on_load(config).unwrap();
        plugin_manager_lock.plugins.push(plugin);
        plugin_manager_lock.libs.push(lib);
        // plugin_manager_lock.libs.push(lib);
        assert_eq!(plugin_manager_lock.plugins[0].name(), DUMMY_NAME);
        plugin_manager_lock.plugins[0].name();

        // Try wrong name (same error)
        const WRONG_NAME: &str = "wrong_name";
        let reload_result = plugin_manager_lock.reload_plugin(WRONG_NAME, DUMMY_CONFIG);
        assert_eq!(
            reload_result.unwrap_err().message,
            "The plugin you requested to reload is not loaded"
        );

        // Now try a (dummy) reload, replacing TestPlugin with TestPlugin2
        let reload_result = plugin_manager_lock.reload_plugin(DUMMY_NAME, DUMMY_CONFIG);
        assert!(reload_result.is_ok());
    }

    #[test]
    fn test_plugin_list() {
        // Initialize empty manager
        let plugin_manager = Arc::new(RwLock::new(GeyserPluginManager::new()));
        let mut plugin_manager_lock = plugin_manager.write().unwrap();

        // Load two plugins
        // First
        let (mut plugin, lib, config) = dummy_plugin_and_library();
        plugin.on_load(config).unwrap();
        plugin_manager_lock.plugins.push(plugin);
        plugin_manager_lock.libs.push(lib);
        // Second
        let (mut plugin, lib, config) = dummy_plugin_and_library2();
        plugin.on_load(config).unwrap();
        plugin_manager_lock.plugins.push(plugin);
        plugin_manager_lock.libs.push(lib);

        // Check that both plugins are returned in the list
        let plugins = plugin_manager_lock.list_plugins().unwrap();
        assert!(plugins.iter().any(|name| name.eq(DUMMY_NAME)));
        assert!(plugins.iter().any(|name| name.eq(ANOTHER_DUMMY_NAME)));
    }

    #[test]
    fn test_plugin_load_unload() {
        // Initialize empty manager
        let plugin_manager = Arc::new(RwLock::new(GeyserPluginManager::new()));
        let mut plugin_manager_lock = plugin_manager.write().unwrap();

        // Load rpc call
        let load_result = plugin_manager_lock.load_plugin(DUMMY_CONFIG);
        assert!(load_result.is_ok());
        assert_eq!(plugin_manager_lock.plugins.len(), 1);

        // Unload rpc call
        let unload_result = plugin_manager_lock.unload_plugin(DUMMY_NAME);
        assert!(unload_result.is_ok());
        assert_eq!(plugin_manager_lock.plugins.len(), 0);
    }
}

impl BpfTracerPluginManager for GeyserPluginManager {
    fn bpf_tracer_plugins(&self) -> Box<dyn Iterator<Item = &Box<dyn BpfTracerPlugin>> + '_> {
        Box::new(self.plugins.iter().filter_map(|plugin| match plugin {
            Plugin::BpfTracer(bpf_tracer_plugin) => Some(bpf_tracer_plugin),
            _ => None,
        }))
    }

    fn active_bpf_tracer_plugins_mut(
        &mut self,
    ) -> Box<dyn Iterator<Item = &mut Box<dyn BpfTracerPlugin>> + '_> {
        Box::new(self.plugins.iter_mut().filter_map(|plugin| match plugin {
            Plugin::BpfTracer(bpf_tracer_plugin) if bpf_tracer_plugin.bpf_tracing_enabled() => {
                Some(bpf_tracer_plugin)
            }
            _ => None,
        }))
    }
}
