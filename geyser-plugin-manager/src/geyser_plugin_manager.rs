use std::{io::Write, thread::JoinHandle, time::Duration};

/// Managing the Geyser plugins
use {
    libloading::{Library, Symbol},
    log::*,
    solana_geyser_plugin_interface::geyser_plugin_interface::GeyserPlugin,
    std::{
        error::Error,
        sync::{Arc, RwLock},
    },
};

type PluginConstructor = unsafe fn() -> *mut dyn GeyserPlugin;

#[derive(Default, Debug)]
pub struct GeyserPluginManager {
    pub plugins: Vec<Arc<RwLock<Box<dyn GeyserPlugin>>>>,
    libs: Vec<Arc<RwLock<Library>>>,
    refreshers: Vec<JoinHandle<()>>,
}

impl GeyserPluginManager {
    pub fn new() -> Self {
        GeyserPluginManager {
            plugins: Vec::default(),
            libs: Vec::default(),
            refreshers: Vec::default(),
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
        // Load the dynamic library and reconstruct the Box<dyn GeyserPlugin>
        let lib = Library::new(libpath)?;
        let constructor: Symbol<PluginConstructor> = lib.get(b"_create_plugin")?;
        let plugin_raw = constructor();
        let mut plugin: Box<dyn GeyserPlugin> = Box::from_raw(plugin_raw);

        // Run on_load to do necessary initializations
        plugin.on_load(config_file)?;

        // Wrap in Arc<RwLock<_>>
        let plugin = Arc::new(RwLock::new(plugin));
        let lib = Arc::new(RwLock::new(lib));

        self.plugins.push(Arc::clone(&plugin));
        self.libs.push(Arc::clone(&lib));

        let owned_path = std::path::PathBuf::from(libpath);
        let owned_config = config_file.to_owned();
        let handle = std::thread::spawn(move || {
            let mut refresh_path = owned_path.clone();
            refresh_path.set_extension("");
            let mut done_path = refresh_path.clone();
            refresh_path.set_file_name(format!(
                "refresh_{}",
                refresh_path
                    .file_name()
                    .map(std::ffi::OsStr::to_str)
                    .expect("valid path with valid filename was given if library loaded")
                    .expect("valid path with valid filename was given if library loaded")
            ));
            done_path.set_file_name(format!(
                "result_{}",
                refresh_path
                    .file_name()
                    .map(std::ffi::OsStr::to_str)
                    .expect("valid path with valid filename was given if library loaded")
                    .expect("valid path with valid filename was given if library loaded")
            ));

            #[allow(unused_must_use)] // result io
            loop {
                // Every 10 seconds..
                std::thread::park_timeout(Duration::from_secs(10));

                info!("checking for new plugin at {}", refresh_path.display());

                // If refresh path exists, handle refresh request
                if refresh_path.exists() {
                    info!("detected refresh_path {}", refresh_path.display());

                    // Initialize file to write result
                    let mut done_file = std::fs::File::create(&done_path)
                        .expect("valid path with valid filename was given if library loaded");

                    // Try load plugin
                    if let Ok(new_lib) = Library::new(&owned_path) {
                        // Try fetching create plugin
                        if let Ok(constructor) = new_lib.get::<PluginConstructor>(b"_create_plugin")
                        {
                            // First construct plugin
                            let new_plugin_raw = constructor();
                            let mut new_plugin: Box<dyn GeyserPlugin> =
                                Box::from_raw(new_plugin_raw);

                            // Overwrite simultaneously
                            let mut plugin_lock = plugin.write().unwrap();
                            let mut lib_lock = lib.write().unwrap();

                            // Try to start up new geyser
                            if new_plugin.on_load(&owned_config).is_ok() {
                                // Shutdown current geyser
                                plugin_lock.on_unload();

                                // Overwrite and release locks
                                *plugin_lock = new_plugin;
                                *lib_lock = new_lib;

                                // Release explicitly for clarity
                                drop(plugin_lock);
                                drop(lib_lock);

                                // Write success
                                done_file.write(b"success");
                            } else {
                                done_file.write(b"created plugin but on_load failed");
                            }
                        } else {
                            done_file.write(b"failed to create_plugin");
                        }
                    } else {
                        done_file.write(b"failed to find plugin");
                    }
                    info!("removed file {}", refresh_path.display());
                    std::fs::remove_file(&refresh_path);
                }
            }
        });

        // Push refresher threads
        self.refreshers.push(handle);

        Ok(())
    }

    /// Unload all plugins and loaded plugin libraries, making sure to fire
    /// their `on_plugin_unload()` methods so they can do any necessary cleanup.
    pub fn unload(&mut self) {
        for plugin in self.plugins.drain(..) {
            let mut plugin = plugin.write().unwrap();
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
            let plugin = plugin.read().unwrap();
            if plugin.account_data_notifications_enabled() {
                return true;
            }
        }
        false
    }

    /// Check if there is any plugin interested in transaction data
    pub fn transaction_notifications_enabled(&self) -> bool {
        for plugin in &self.plugins {
            let plugin = plugin.read().unwrap();
            if plugin.transaction_notifications_enabled() {
                return true;
            }
        }
        false
    }
}
