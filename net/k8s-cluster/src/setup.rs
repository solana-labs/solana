use clap::ArgSettings;


use {
    super::initialize_globals,
    crate::{
        cat_file,
        extract_release_archive,
        download_to_temp,
        SOLANA_ROOT,
    },
    log::*,
    std::{
        fs,
        path::PathBuf,
    },
    chrono::prelude::*,
};

#[derive(Clone, Debug)]
pub struct BuildConfig<'a> {
    pub release_channel: &'a str,
    pub deploy_method: &'a str,
    pub do_build: bool,
    pub debug_build: bool,
    pub profile_build: bool,
}

#[derive(Clone, Debug)]
pub struct Deploy<'a> {
    config: BuildConfig<'a>,
}

impl<'a> Deploy<'a> {
    pub fn new(config: BuildConfig<'a>) -> Self {
        initialize_globals();
        Deploy { config }
    }

    pub async fn prepare(&self) {
        match self.config.deploy_method {
            "tar" => {
                let file_name = "solana-release";
                match self.setup_tar_deploy(file_name).await {
                    Ok(tar_directory) => {
                        info!("Sucessfuly setup tar file");
                        cat_file(&tar_directory.join("version.yml")).unwrap();
                    },
                    Err(_) => error!("Failed to setup tar file!"),
                }
            }
            "local" => self.setup_local_deploy(),
            "skip" => (),
            _ => error!(
                "Internal error: Invalid deploy_method: {}",
                self.config.deploy_method
            ),
        }
    }

    async fn setup_tar_deploy(&self, file_name: &str) -> Result<PathBuf, String> {
        info!("tar file deploy");
        let tar_file = format!("{}{}", file_name, ".tar.bz2");
        if !self.config.release_channel.is_empty() {
            match self.download_release_from_channel(file_name).await {
                Ok(_) => info!("Successfully downloaded tar release from channel"),
                Err(_) => error!("Failed to download tar release"),
            }
        }

        // Extract it and load the release version metadata
        let tarball_filename = super::SOLANA_ROOT.join(tar_file);
        let temp_release_dir = super::SOLANA_ROOT.join(file_name);
        extract_release_archive(&tarball_filename, &temp_release_dir, file_name).map_err(|err| {
            format!("Unable to extract {tarball_filename:?} into {temp_release_dir:?}: {err}")
        })?;

        Ok(temp_release_dir)
    }

    fn setup_local_deploy(&self) {
        info!("local deploy");
        if self.config.do_build {
            info!("call build()");
            self.build();
        } else {
            info!("Build skipped due to --no-build");
        }
    }

    fn build(&self) {
        info!("building!");
        let local: DateTime<Local> = Local::now();
        info!("--- Build started at {}", local.format("%Y-%m-%d %H:%M"));
        let build_variant: &str = if self.config.debug_build { "--debug" } else { "" };
        if self.config.profile_build {
            // format!("{}{:?}{}", "RUSTFLAGS='-C force-frame-pointers=y -g ", super::RUSTFLAGS, "'")
            // format!("{}", "RUSTFLAGS='-C force-frame-pointers=y -g '")

            // let rustflags = "-C force-frame-pointers=y -D warnings";
            info!("rust flags in build: {}", *super::RUST_FLAGS);
            let rustflags = format!("{}{}{}", "RUSTFLAGS='-C force-frame-pointers=y -g ",  *super::RUST_FLAGS, "'");
            info!("rust flags: {}", rustflags);
            std::env::set_var("RUSTFLAGS", rustflags);
            info!("rust flags updated: {}", std::env::var("RUSTFLAGS").ok().unwrap());
        }

        let cargo_install_all_path = super::SOLANA_ROOT.join("scripts/cargo-install-all.sh");
        // let command = PathBuf::from(format!("{} {:?}", profiler_flags, cargo_install_all_path));
        let install_directory = super::SOLANA_ROOT.join("farf");
        let arguments = format!("{:?} {} {}", install_directory, build_variant, "--validator-only");

        // info!("command: {:?}", command);
        info!("args: {}", arguments);

        let status = std::process::Command::new("./cargo-install-all.sh")
            .current_dir(super::SOLANA_ROOT.join("scripts"))
            .arg(arguments)
            .status()
            .expect("Failed to build validator executable");


        // info!("profiler_flags: {}", profiler_flags);
        // let rustflags = "-C force-frame-pointers=y";
        // std::env::set_var("RUSTFLAGS", rustflags);

        // let command = PathBuf::from(format!("{} {:?}", profiler_flags, "ls"));
        // let command = PathBuf::from(format!("{}", "ls"));
        // let status = std::process::Command::new(command)
        //     .current_dir("scripts")
        //     .status()
        //     .expect("Failed to build validator executable");
        
        if status.success() {
            info!("successfully build validator binary");
        } else {
            error!("Failed to build executable!");
        }
        






    }

    async fn download_release_from_channel(&self, file_name: &str) -> Result<(), String> {
        info!(
            "Downloading release from channel: {}",
            self.config.release_channel
        );
        let tar_file = format!("{}{}", file_name, ".tar.bz2");
        let file_path = super::SOLANA_ROOT.join(tar_file.as_str());
        // Remove file
        if let Err(err) = fs::remove_file(&file_path) {
            if err.kind() != std::io::ErrorKind::NotFound {
                error!("Error while removing file: {:?}", err);
            }
        }

        let update_download_url = format!(
            "{}{}{}",
            "https://release.solana.com/",
            self.config.release_channel,
            "/solana-release-x86_64-unknown-linux-gnu.tar.bz2"
        );
        info!("update_download_url: {}", update_download_url);

        let _ = download_to_temp(update_download_url.as_str(), tar_file.as_str())
            .await
            .map_err(|err| format!("Unable to download {update_download_url}: {err}"))?;

        Ok(())
    }

    // async
}
