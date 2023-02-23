use std::{thread::JoinHandle, time::Duration};

use {
    crate::{
        accounts_update_notifier::AccountsUpdateNotifierImpl,
        block_metadata_notifier::BlockMetadataNotifierImpl,
        block_metadata_notifier_interface::BlockMetadataNotifierLock,
        geyser_plugin_manager::GeyserPluginManager, slot_status_notifier::SlotStatusNotifierImpl,
        slot_status_observer::SlotStatusObserver, transaction_notifier::TransactionNotifierImpl,
    },
    crossbeam_channel::Receiver,
    jsonrpc_core::{ErrorCode, Result as JsonRpcResult},
    jsonrpc_server_utils::tokio::sync::oneshot::Sender as OneShotSender,
    log::*,
    solana_geyser_plugin_interface::geyser_plugin_interface::GeyserPlugin,
    solana_rpc::{
        optimistically_confirmed_bank_tracker::BankNotification,
        transaction_notifier_interface::TransactionNotifierLock,
    },
    solana_runtime::accounts_update_notifier_interface::AccountsUpdateNotifier,
    std::{
        path::{Path, PathBuf},
        sync::{Arc, RwLock},
        thread,
    },
    thiserror::Error,
};

#[derive(Error, Debug)]
pub enum GeyserPluginServiceError {
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
}

/// The service managing the Geyser plugin workflow.
pub struct GeyserPluginService {
    slot_status_observer: Option<SlotStatusObserver>,
    plugin_manager: Arc<RwLock<GeyserPluginManager>>,
    accounts_update_notifier: Option<AccountsUpdateNotifier>,
    transaction_notifier: Option<TransactionNotifierLock>,
    block_metadata_notifier: Option<BlockMetadataNotifierLock>,
    #[allow(unused)]
    rpc_handler_thread: Option<Arc<JoinHandle<()>>>,
}

impl GeyserPluginService {
    /// Creates and returns the GeyserPluginService.
    /// # Arguments
    /// * `confirmed_bank_receiver` - The receiver for confirmed bank notification
    /// * `geyser_plugin_config_file` - The config file path for the plugin. The
    ///    config file controls the plugin responsible
    ///    for transporting the data to external data stores. It is defined in JSON format.
    ///    The `libpath` field should be pointed to the full path of the dynamic shared library
    ///    (.so file) to be loaded. The shared library must implement the `GeyserPlugin`
    ///    trait. And the shared library shall export a `C` function `_create_plugin` which
    ///    shall create the implementation of `GeyserPlugin` and returns to the caller.
    ///    The rest of the JSON fields' definition is up to to the concrete plugin implementation
    ///    It is usually used to configure the connection information for the external data store.

    pub fn new(
        confirmed_bank_receiver: Receiver<BankNotification>,
        geyser_plugin_config_files: &[PathBuf],
        rpc_to_plugin_manager_receiver: Option<Receiver<PluginManagerRequest>>,
    ) -> Result<Self, GeyserPluginServiceError> {
        info!(
            "Starting GeyserPluginService from config files: {:?}",
            geyser_plugin_config_files
        );
        let mut plugin_manager = GeyserPluginManager::new();

        for geyser_plugin_config_file in geyser_plugin_config_files {
            Self::initialize_plugin(&mut plugin_manager, geyser_plugin_config_file)?;
        }

        let account_data_notifications_enabled =
            plugin_manager.account_data_notifications_enabled();
        let transaction_notifications_enabled = plugin_manager.transaction_notifications_enabled();
        let plugin_manager = Arc::new(RwLock::new(plugin_manager));

        let accounts_update_notifier: Option<AccountsUpdateNotifier> =
            if account_data_notifications_enabled {
                let accounts_update_notifier =
                    AccountsUpdateNotifierImpl::new(plugin_manager.clone());
                Some(Arc::new(RwLock::new(accounts_update_notifier)))
            } else {
                None
            };

        let transaction_notifier: Option<TransactionNotifierLock> =
            if transaction_notifications_enabled {
                let transaction_notifier = TransactionNotifierImpl::new(plugin_manager.clone());
                Some(Arc::new(RwLock::new(transaction_notifier)))
            } else {
                None
            };

        let (slot_status_observer, block_metadata_notifier): (
            Option<SlotStatusObserver>,
            Option<BlockMetadataNotifierLock>,
        ) = if account_data_notifications_enabled || transaction_notifications_enabled {
            let slot_status_notifier = SlotStatusNotifierImpl::new(plugin_manager.clone());
            let slot_status_notifier = Arc::new(RwLock::new(slot_status_notifier));
            (
                Some(SlotStatusObserver::new(
                    confirmed_bank_receiver,
                    slot_status_notifier,
                )),
                Some(Arc::new(RwLock::new(BlockMetadataNotifierImpl::new(
                    plugin_manager.clone(),
                )))),
            )
        } else {
            (None, None)
        };

        let rpc_handler_thread = rpc_to_plugin_manager_receiver.map(|request_receiver| {
            let rpc_plugin_manager = plugin_manager.clone();
            Arc::new(std::thread::spawn(move || loop {
                std::thread::park();
                info!("we've been awakened");
                // Wait 3 seconds to receive message after being awoken
                if let Ok(request) = request_receiver.recv_timeout(Duration::from_secs(3)) {
                    match request {
                        PluginManagerRequest::ListPlugins { response_sender } => {
                            let plugin_manager = rpc_plugin_manager.read().unwrap();
                            response_sender
                                .send(Self::list_plugins_rpc(&plugin_manager))
                                .expect("admin rpc service will be waiting for response");
                        }

                        PluginManagerRequest::ReloadPlugin {
                            ref name,
                            ref config_file,
                            response_sender,
                        } => {
                            let mut plugin_manager = rpc_plugin_manager.write().unwrap();
                            response_sender
                                .send(Self::reload_plugin_rpc(
                                    &mut plugin_manager,
                                    name,
                                    config_file,
                                ))
                                .expect("admin rpc service will be waiting for response");
                        }

                        PluginManagerRequest::LoadPlugin {
                            ref config_file,
                            response_sender,
                        } => {
                            let mut plugin_manager = rpc_plugin_manager.write().unwrap();
                            response_sender
                                .send(Self::load_plugin_rpc(&mut plugin_manager, config_file))
                                .expect("admin rpc service will be waiting for response");
                        }

                        PluginManagerRequest::UnloadPlugin {
                            ref name,
                            response_sender,
                        } => {
                            let mut plugin_manager = rpc_plugin_manager.write().unwrap();
                            response_sender
                                .send(Self::unload_plugin_rpc(&mut plugin_manager, name))
                                .expect("admin rpc service will be waiting for response");
                        }
                    }
                }
            }))
        });

        info!("Started GeyserPluginService");
        Ok(GeyserPluginService {
            slot_status_observer,
            plugin_manager,
            accounts_update_notifier,
            transaction_notifier,
            block_metadata_notifier,
            rpc_handler_thread,
        })
    }

    fn initialize_plugin(
        plugin_manager: &mut GeyserPluginManager,
        geyser_plugin_config_file: &Path,
    ) -> Result<(), GeyserPluginServiceError> {
        let plugin: Box<dyn GeyserPlugin> = load_plugin_from_config(geyser_plugin_config_file)?;
        plugin_manager.plugins.push(plugin);
        Ok(())
    }

    pub fn get_accounts_update_notifier(&self) -> Option<AccountsUpdateNotifier> {
        self.accounts_update_notifier.clone()
    }

    pub fn get_transaction_notifier(&self) -> Option<TransactionNotifierLock> {
        self.transaction_notifier.clone()
    }

    pub fn get_block_metadata_notifier(&self) -> Option<BlockMetadataNotifierLock> {
        self.block_metadata_notifier.clone()
    }

    pub fn join(self) -> thread::Result<()> {
        if let Some(mut slot_status_observer) = self.slot_status_observer {
            slot_status_observer.join()?;
        }
        self.plugin_manager.write().unwrap().unload();
        Ok(())
    }

    pub fn plugin_manager_join_handle(&self) -> Arc<JoinHandle<()>> {
        // This is only called after the initialization of a solana-test-validator or a solana-validator
        // which has a geyser plugin service
        Arc::clone(self.rpc_handler_thread.as_ref().unwrap())
    }

    pub fn list_plugins_rpc(plugin_manager: &GeyserPluginManager) -> JsonRpcResult<Vec<String>> {
        Ok(plugin_manager
            .plugins
            .iter()
            .map(|p| p.name().to_owned())
            .collect())
    }

    pub fn load_plugin_rpc(
        plugin_manager: &mut GeyserPluginManager,
        config_file: &str,
    ) -> JsonRpcResult<String> {
        // Load plugin
        let mut new_plugin: Box<dyn GeyserPlugin> = load_plugin_from_config(Path::new(config_file))
            .map_err(|err| jsonrpc_core::Error {
                code: ErrorCode::InternalError,
                message: err.to_string(),
                data: None,
            })?;

        // Then see if a plugin with this name already exists. If so, abort
        if plugin_manager
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
        plugin_manager.plugins.push(new_plugin);

        Ok(name)
    }

    pub fn unload_plugin_rpc(
        plugin_manager: &mut GeyserPluginManager,
        name: &str,
    ) -> JsonRpcResult<()> {
        // Check if any plugin names match this one
        let Some(idx) = plugin_manager.plugins.iter().position(|plugin| plugin.name().eq(name)) else {
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
        let mut current_plugin = plugin_manager.plugins.remove(idx);
        current_plugin.on_unload();
        drop(current_plugin);

        Ok(())
    }

    /// Checks for a plugin with a given `name`.
    /// If it exists, first unload it.
    /// Then, attempt to load a new plugin
    pub fn reload_plugin_rpc(
        plugin_manager: &mut GeyserPluginManager,
        name: &str,
        config_file: &str,
    ) -> JsonRpcResult<()> {
        // Check if any plugin names match this one
        let Some(idx) = plugin_manager.plugins.iter().position(|plugin| plugin.name().eq(name)) else {
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
        let mut current_plugin = plugin_manager.plugins.remove(idx);
        current_plugin.on_unload();
        drop(current_plugin);

        // Try to load plugin, library
        // SAFETY: It is up to the validator to ensure this is a valid plugin library.
        let mut new_plugin: Box<dyn GeyserPlugin> = load_plugin_from_config(Path::new(config_file))
            .map_err(|err| jsonrpc_core::Error {
                code: ErrorCode::InternalError,
                message: err.to_string(),
                data: None,
            })?;

        // Attempt to on_load with new plugin
        match new_plugin.on_load(&config_file) {
            // On success, replace plugin and library
            Ok(()) => {
                plugin_manager.plugins.push(new_plugin);
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

#[cfg(not(test))]
fn load_plugin_from_config(
    geyser_plugin_config_file: &Path,
) -> Result<Box<dyn GeyserPlugin>, GeyserPluginServiceError> {
    use std::fs::File;
    use std::io::Read;

    let mut file = match File::open(geyser_plugin_config_file) {
        Ok(file) => file,
        Err(err) => {
            return Err(GeyserPluginServiceError::CannotOpenConfigFile(format!(
                "Failed to open the plugin config file {geyser_plugin_config_file:?}, error: {err:?}"
            )));
        }
    };

    let mut contents = String::new();
    if let Err(err) = file.read_to_string(&mut contents) {
        return Err(GeyserPluginServiceError::CannotReadConfigFile(format!(
            "Failed to read the plugin config file {geyser_plugin_config_file:?}, error: {err:?}"
        )));
    }

    let result: serde_json::Value = match json5::from_str(&contents) {
        Ok(value) => value,
        Err(err) => {
            return Err(GeyserPluginServiceError::InvalidConfigFileFormat(format!(
                "The config file {geyser_plugin_config_file:?} is not in a valid Json5 format, error: {err:?}"
            )));
        }
    };

    let libpath = result["libpath"]
        .as_str()
        .ok_or(GeyserPluginServiceError::LibPathNotSet)?;
    let mut libpath = PathBuf::from(libpath);
    if libpath.is_relative() {
        let config_dir = geyser_plugin_config_file.parent().ok_or_else(|| {
            GeyserPluginServiceError::CannotOpenConfigFile(format!(
                "Failed to resolve parent of {geyser_plugin_config_file:?}",
            ))
        })?;
        libpath = config_dir.join(libpath);
    }

    let config_file = geyser_plugin_config_file
        .as_os_str()
        .to_str()
        .ok_or(GeyserPluginServiceError::InvalidPluginPath)?;

    unsafe {
        GeyserPluginManager::load_plugin(libpath.to_str().unwrap(), config_file).map_err(|err| {
            let msg = format!("Failed to load the plugin library: {libpath:?}, error: {err:?}");
            GeyserPluginServiceError::PluginLoadError(msg)
        })
    }
}

// This is mocked for tests to avoid having to do IO with a dynamically linked library
// across different architectures at test time
#[cfg(test)]
fn load_plugin_from_config(
    _geyser_plugin_config_file: &Path,
) -> Result<Box<dyn GeyserPlugin>, GeyserPluginServiceError> {
    Ok(tests::dummy_plugin())
}

#[derive(Debug)]
pub enum PluginManagerRequest {
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

#[cfg(test)]
mod tests {
    use std::sync::{Arc, RwLock};

    use crate::{
        geyser_plugin_manager::GeyserPluginManager, geyser_plugin_service::GeyserPluginService,
    };
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

    const DUMMY_NAME: &'static str = "dummy";
    const DUMMY_CONFIG_FILE: &'static str = "dummy_config";
    const ANOTHER_DUMMY_NAME: &'static str = "another_dummy";

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
        let reload_result = GeyserPluginService::reload_plugin_rpc(
            &mut plugin_manager_lock,
            DUMMY_NAME,
            DUMMY_CONFIG_FILE,
        );
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
        let reload_result = GeyserPluginService::reload_plugin_rpc(
            &mut plugin_manager_lock,
            WRONG_NAME,
            DUMMY_CONFIG_FILE,
        );
        assert_eq!(
            reload_result.unwrap_err().message,
            "plugin requested to reload is not loaded"
        );

        // Now try a (dummy) reload, replacing TestPlugin with TestPlugin2
        let reload_result = GeyserPluginService::reload_plugin_rpc(
            &mut plugin_manager_lock,
            DUMMY_NAME,
            DUMMY_CONFIG_FILE,
        );
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
        let mut plugin = dummy_plugin2();
        plugin.on_load(DUMMY_CONFIG_FILE).unwrap();
        plugin_manager_lock.plugins.push(plugin);

        // Check that both plugins are returned in the list
        let plugins = GeyserPluginService::list_plugins_rpc(&plugin_manager_lock).unwrap();
        assert!(plugins.iter().any(|name| name.eq(DUMMY_NAME)));
        assert!(plugins.iter().any(|name| name.eq(ANOTHER_DUMMY_NAME)));
    }

    #[test]
    fn test_plugin_load_unload() {
        // Initialize empty manager
        let plugin_manager = Arc::new(RwLock::new(GeyserPluginManager::new()));
        let mut plugin_manager_lock = plugin_manager.write().unwrap();

        // Load rpc call
        let load_result =
            GeyserPluginService::load_plugin_rpc(&mut plugin_manager_lock, DUMMY_CONFIG_FILE);
        assert!(load_result.is_ok());
        assert_eq!(plugin_manager_lock.plugins.len(), 1);

        // Unload rpc call
        let unload_result =
            GeyserPluginService::unload_plugin_rpc(&mut plugin_manager_lock, DUMMY_NAME);
        assert!(unload_result.is_ok());
        assert_eq!(plugin_manager_lock.plugins.len(), 0);
    }
}
