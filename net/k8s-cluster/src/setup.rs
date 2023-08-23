use {
    log::*,
    super::initialize_globals,
    std::{
        fs,
        fs::File,
        io,
    },
    reqwest
};

#[derive(Clone, Debug)]
pub struct DeployConfig<'a> {

    pub release_channel: &'a str,
    pub deploy_method: &'a str,
    pub do_build: bool,

}

#[derive(Clone, Debug)]
pub struct Deploy<'a> {
    config: DeployConfig<'a>,
}



impl<'a> Deploy<'a> {
    pub fn new(
        config: DeployConfig<'a>
    ) -> Self {
        initialize_globals();
        Deploy {
            config
        }
    }

    pub async fn prepare(
        &self,
    ) {
        match self.config.deploy_method {
            "tar" => self.setup_tar_deploy().await,
            "local" => self.setup_local_deploy(),
            "skip" => (),
            _ => error!("Internal error: Invalid deploy_method: {}", self.config.deploy_method),
        }
    }

    async fn setup_tar_deploy(
        &self,
    ) {
        info!("tar file deploy");
        if !self.config.release_channel.is_empty() {
            // self.download_release_from_channel().await;
            match self.download_release_from_channel().await {
                Ok(_) => info!("Successfully downloaded tar release from channel"),
                Err(_) => error!("Failed to download tar release")
            }
        } else {
            info!("release channel is empty!");
        }
    }

    fn setup_local_deploy(
        &self,
    ) {
        info!("local deploy");
        if self.config.do_build {
            info!("call build()");
            self.build();
        } else {
            info!("Build skipped due to --no-build");
        }
        
    }

    fn build(
        &self,
    ) {
        info!("building!");
    }

    async fn download_release_from_channel(
        &self,
    ) -> Result<(), reqwest::Error> {
        info!("Downloading release from channel: {}", self.config.release_channel);
        let file_path = super::SOLANA_ROOT.join("solana-release.tar.bz2");
        // Remove file
        if let Err(err) = fs::remove_file(&file_path) {
            if err.kind() != std::io::ErrorKind::NotFound {
                error!("Error while removing file: {:?}", err);
            }
        }

        let update_download_url = format!("{}{}{}", "https://release.solana.com/", self.config.release_channel, "/solana-release-x86_64-unknown-linux-gnu.tar.bz2");
        info!("update_download_url: {}", update_download_url);

        let file_name = super::SOLANA_ROOT.join("solana-release.tar.bz2");
        info!("resulting filename: {:?}", file_name);
        let response = reqwest::get(&update_download_url).await.expect("request failed");
        let body = response.text().await.expect("body invalid");
        let mut out = File::create(file_name).expect("failed to create file");
        io::copy(&mut body.as_bytes(), &mut out).expect("failed to copy content");

       
        Ok(())

    }

    // async
}

