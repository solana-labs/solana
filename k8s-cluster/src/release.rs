use {
    super::initialize_globals,
    crate::{boxed_error, cat_file, download_to_temp, extract_release_archive, SOLANA_ROOT},
    git2::Repository,
    log::*,
    std::{error::Error, fs, path::PathBuf, time::Instant},
};

#[derive(Clone, Debug)]
pub struct BuildConfig<'a> {
    release_channel: &'a str,
    deploy_method: &'a str,
    do_build: bool,
    debug_build: bool,
    profile_build: bool,
    docker_build: bool,
    build_path: PathBuf,
}

impl<'a> BuildConfig<'a> {
    pub fn new(
        release_channel: &'a str,
        deploy_method: &'a str,
        do_build: bool,
        debug_build: bool,
        profile_build: bool,
        docker_build: bool,
    ) -> Self {
        let build_path = if deploy_method == "local" {
            SOLANA_ROOT.join("farf/bin")
        } else if deploy_method == "tar" {
            SOLANA_ROOT.join("solana-release/bin")
        } else if deploy_method == "skip" {
            SOLANA_ROOT.join("farf/bin")
        } else {
            warn!("deploy_method is invalid: {}", deploy_method);
            SOLANA_ROOT.join("")
        };

        BuildConfig {
            release_channel,
            deploy_method,
            do_build,
            debug_build,
            profile_build,
            docker_build,
            build_path,
        }
    }

    pub fn deploy_method(&self) -> &str {
        self.deploy_method
    }

    pub fn docker_build(&self) -> bool {
        self.docker_build
    }

    pub fn build_path(&self) -> PathBuf {
        self.build_path.clone()
    }
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

    pub async fn prepare(&self) -> Result<(), Box<dyn Error>> {
        match self.config.deploy_method {
            "tar" => {
                let file_name = "solana-release";
                match self.setup_tar_deploy(file_name).await {
                    Ok(tar_directory) => {
                        info!("Sucessfuly setup tar file");
                        cat_file(&tar_directory.join("version.yml")).unwrap();
                    }
                    Err(err) => {
                        error!("Failed to setup tar file! Did you set --release-channel? \
                        Or is there a solana-release.tar.bz2 file already in your solana/ root directory?");
                        return Err(err);
                    }
                }
            }
            "local" => {
                match self.setup_local_deploy() {
                    Ok(_) => (),
                    Err(err) => return Err(err),
                };
            }
            "skip" => (),
            _ => {
                let error = format!(
                    "{} {}",
                    "Internal error: Invalid deploy_method:", self.config.deploy_method
                );
                return Err(boxed_error!(error));
            }
        }
        info!("Completed Prepare Deploy");
        Ok(())
    }

    async fn setup_tar_deploy(&self, file_name: &str) -> Result<PathBuf, Box<dyn Error>> {
        let tar_file = format!("{}{}", file_name, ".tar.bz2");
        if !self.config.release_channel.is_empty() {
            match self.download_release_from_channel(file_name).await {
                Ok(_) => info!("Successfully downloaded tar release from channel"),
                Err(_) => error!("Failed to download tar release"),
            }
        } else {
            info!("No release channel set. Attempting to extract a local version of solana-release.tar.bz2...");
        }

        // Extract it and load the release version metadata
        let tarball_filename = SOLANA_ROOT.join(tar_file);
        let temp_release_dir = SOLANA_ROOT.join(file_name);
        extract_release_archive(&tarball_filename, &temp_release_dir, file_name).map_err(
            |err| {
                format!("Unable to extract {tarball_filename:?} into {temp_release_dir:?}: {err}")
            },
        )?;

        Ok(temp_release_dir)
    }

    fn setup_local_deploy(&self) -> Result<(), Box<dyn Error>> {
        if self.config.do_build {
            match self.build() {
                Ok(_) => (),
                Err(err) => return Err(err),
            };
        } else {
            info!("Build skipped due to --no-build");
        }
        Ok(())
    }

    fn build(&self) -> Result<(), Box<dyn Error>> {
        let start_time = Instant::now();
        let build_variant: &str = if self.config.debug_build {
            "--debug"
        } else {
            ""
        };
        if self.config.profile_build {
            return Err(boxed_error!("Profile Build not implemented yet"));
        }

        let install_directory = SOLANA_ROOT.join("farf");
        match std::process::Command::new("./cargo-install-all.sh")
            .current_dir(SOLANA_ROOT.join("scripts"))
            .arg(install_directory)
            .arg(build_variant)
            .arg("--validator-only")
            .status()
        {
            Ok(result) => {
                if result.success() {
                    info!("Successfully build validator")
                } else {
                    return Err(boxed_error!("Failed to build validator"));
                }
            }
            Err(err) => return Err(Box::new(err)),
        }

        let solana_repo =
            Repository::open(SOLANA_ROOT.as_path()).expect("Failed to open repository");
        let commit = solana_repo.revparse_single("HEAD").unwrap().id();
        let branch = solana_repo
            .head()
            .expect("Failed to get branch for HEAD")
            .shorthand()
            .expect("Failed to get shortened branch name")
            .to_string();

        // Check if current commit is associated with a tag
        let mut note = branch;
        for tag in (&solana_repo
            .tag_names(None)
            .expect("Failed to retrieve tag names"))
            .into_iter()
            .flatten()
        {
            // Get the target object of the tag
            let tag_object = solana_repo
                .revparse_single(tag)
                .expect("Failed to parse tag")
                .id();
            // Check if the commit associated with the tag is the same as the current commit
            if tag_object == commit {
                info!("The current commit is associated with tag: {}", tag);
                note = tag_object.to_string();
                break;
            }
        }

        // Write to branch/tag and commit to version.yml
        let content = format!("channel: devbuild {}\ncommit: {}", note, commit);
        std::fs::write(SOLANA_ROOT.join("farf/version.yml"), content)
            .expect("Failed to write version.yml");

        info!("Build took {:.3?} seconds", start_time.elapsed());
        Ok(())
    }

    async fn download_release_from_channel(&self, file_name: &str) -> Result<(), Box<dyn Error>> {
        info!(
            "Downloading release from channel: {}",
            self.config.release_channel
        );
        let tar_file = format!("{}{}", file_name, ".tar.bz2");
        let file_path = SOLANA_ROOT.join(tar_file.as_str());
        // Remove file
        if let Err(err) = fs::remove_file(&file_path) {
            if err.kind() != std::io::ErrorKind::NotFound {
                return Err(boxed_error!(format!(
                    "{}: {:?}",
                    "Error while removing file:", err
                )));
            }
        }

        let update_download_url = format!(
            "{}{}{}",
            "https://release.solana.com/",
            self.config.release_channel,
            "/solana-release-x86_64-unknown-linux-gnu.tar.bz2"
        );
        info!("update_download_url: {}", update_download_url);

        download_to_temp(update_download_url.as_str(), tar_file.as_str())
            .await
            .map_err(|err| format!("Unable to download {update_download_url}: {err}"))?;

        Ok(())
    }
}
