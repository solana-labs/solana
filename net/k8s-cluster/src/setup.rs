use clap::ArgSettings;


use {
    chrono::prelude::*,
    crate::{
        cat_file,
        extract_release_archive,
        download_to_temp,
        SOLANA_ROOT,
    },
    git2::{Repository, DescribeOptions, DescribeFormatOptions},
    log::*,
    std::{
        fs,
        path::PathBuf,
        time::Instant,
    },
    super::initialize_globals,
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
        let start_time = Instant::now();
        let build_variant: &str = if self.config.debug_build { "--debug" } else { "" };
        if self.config.profile_build {
            error!("Profile Build not implemented yet");
            // info!("rust flags in build: {}", *super::RUST_FLAGS);
            // let rustflags = format!("{}{}{}", "RUSTFLAGS='-C force-frame-pointers=y -g ",  *super::RUST_FLAGS, "'");
            // info!("rust flags: {}", rustflags);
            // std::env::set_var("RUSTFLAGS", rustflags);
            // info!("rust flags updated: {}", std::env::var("RUSTFLAGS").ok().unwrap());
        }

        let install_directory = super::SOLANA_ROOT.join("farf");
        let status = std::process::Command::new("./cargo-install-all.sh")
            .current_dir(super::SOLANA_ROOT.join("scripts"))
            .arg(install_directory)
            .arg(build_variant)
            .arg("--validator-only")
            .status()
            .expect("Failed to build validator executable");
        
        if status.success() {
            info!("successfully build validator binary");
        } else {
            error!("Failed to build executable!");
        }
        
        let solana_repo = Repository::open(super::SOLANA_ROOT.as_path()).expect("Failed to open repository");
        let commit = solana_repo
            .revparse_single("HEAD")
            .unwrap()
            .id();
        let branch = solana_repo
            .head()
            .expect("Failed to get branch for HEAD")
            .shorthand()
            .expect("Failed to get shortened branch name")
            .to_string();
        // unwrap().name().unwrap_or_default().to_string();

        let mut note = branch;
        // Iterate over all tags in the repository
        for tag in &solana_repo.tag_names(None).expect("Failed to retrieve tag names") {
            if let Some(tag_name) = tag {
                // Get the target object of the tag
                let tag_object = solana_repo.revparse_single(&tag_name).expect("Failed to parse tag").id();
                // Check if the commit associated with the tag is the same as the current commit
                if tag_object == commit {
                    println!("The current commit is associated with tag: {}", tag_name);
                    note = tag_object.to_string();
                    break;
                } 
            }
        }
        info!("commit, note: {}, {}", commit, note);

        // Write to version.yml
        let content = format!(
            "channel: devbuild {}\ncommit: {}",
            note,
            commit.to_string()
        );
        std::fs::write(super::SOLANA_ROOT.join("farf/version.yml"), content)
            .expect("Failed to write version.yml");
        
 
        info!("Build took {:.3?} seconds", start_time.elapsed());

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
