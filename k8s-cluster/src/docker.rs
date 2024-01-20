use {
    crate::{
        boxed_error, initialize_globals, new_spinner_progress_bar, ValidatorType, BUILD, ROCKET,
        SOLANA_ROOT,
    },
    log::*,
    rayon::prelude::*,
    std::{
        error::Error,
        fs,
        path::PathBuf,
        process::{Command, Output, Stdio},
    },
};

#[derive(Clone, Debug)]
pub struct DockerImageConfig<'a> {
    pub base_image: &'a str,
    pub image_name: &'a str,
    pub tag: &'a str,
    pub registry: &'a str,
}

pub struct DockerConfig<'a> {
    image_config: DockerImageConfig<'a>,
    deploy_method: &'a str,
}

impl<'a> DockerConfig<'a> {
    pub fn new(image_config: DockerImageConfig<'a>, deploy_method: &'a str) -> Self {
        initialize_globals();
        DockerConfig {
            image_config,
            deploy_method,
        }
    }

    pub fn build_image(&self, validator_type: &ValidatorType) -> Result<(), Box<dyn Error>> {
        let image_name = format!("{}-{}", validator_type, self.image_config.image_name);
        let docker_path = SOLANA_ROOT.join(format!("{}/{}", "docker-build", validator_type));
        match self.create_base_image(image_name, docker_path, validator_type, None) {
            Ok(res) => {
                if res.status.success() {
                    info!("Successfully created base Image");
                    Ok(())
                } else {
                    error!("Failed to build base image");
                    Err(boxed_error!(String::from_utf8_lossy(&res.stderr)))
                }
            }
            Err(err) => Err(err),
        }
    }

    pub fn build_client_images(&self, client_count: i32) -> Result<(), Box<dyn Error>> {
        for i in 0..client_count {
            let image_name = format!("{}-{}-{}", ValidatorType::Client, "image", i);
            let docker_path = SOLANA_ROOT.join(format!(
                "{}/{}-{}",
                "docker-build",
                ValidatorType::Client,
                i
            ));
            match self.create_base_image(image_name, docker_path, &ValidatorType::Client, Some(i)) {
                Ok(res) => {
                    if res.status.success() {
                        info!("Successfully created client base Image: {}", i);
                    } else {
                        error!("Failed to build client base image: {}", i);
                        return Err(boxed_error!(String::from_utf8_lossy(&res.stderr)));
                    }
                }
                Err(err) => return Err(err),
            }
        }
        Ok(())
    }

    pub fn create_base_image(
        &self,
        image_name: String,
        docker_path: PathBuf,
        validator_type: &ValidatorType,
        index: Option<i32>,
    ) -> Result<Output, Box<dyn Error>> {
        let dockerfile_path = match self.create_dockerfile(validator_type, docker_path, None, index)
        {
            Ok(res) => res,
            Err(err) => return Err(err),
        };

        trace!("Tmp: {}", dockerfile_path.as_path().display());
        trace!("Exists: {}", dockerfile_path.as_path().exists());

        // We use std::process::Command here because Docker-rs is very slow building dockerfiles
        // when they are in large repos. Docker-rs doesn't seem to support the `--file` flag natively.
        // so we result to using std::process::Command
        let dockerfile = dockerfile_path.join("Dockerfile");
        let context_path = SOLANA_ROOT.display().to_string();

        let progress_bar = new_spinner_progress_bar();
        progress_bar.set_message(format!(
            "{BUILD}Building {} docker image...",
            validator_type
        ));

        let command = format!(
            "docker build -t {}/{}:{} -f {:?} {}",
            self.image_config.registry, image_name, self.image_config.tag, dockerfile, context_path
        );
        let output = match Command::new("sh")
            .arg("-c")
            .arg(&command)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("Failed to execute command")
            .wait_with_output()
        {
            Ok(res) => Ok(res),
            Err(err) => Err(Box::new(err) as Box<dyn Error>),
        };
        progress_bar.finish_and_clear();
        info!("{} image build complete", validator_type);

        output
    }

    fn insert_client_accounts_if_present(&self, index: Option<i32>) -> String {
        let return_string = match index {
            Some(i) => {
                if SOLANA_ROOT
                    .join(format!("config-k8s/bench-tps-{}.yml", i))
                    .exists()
                {
                    format!(
                        r#"
        COPY --chown=solana:solana ./config-k8s/bench-tps-{}.yml /home/solana/client-accounts.yml
                    "#,
                        i
                    )
                } else {
                    info!("bench-tps-{} does not exist!", i);
                    "".to_string()
                }
            }
            None => {
                if SOLANA_ROOT.join("config-k8s/client-accounts.yml").exists() {
                    r#"
        COPY --chown=solana:solana ./config-k8s/client-accounts.yml /home/solana
                    "#
                    .to_string()
                } else {
                    "".to_string()
                }
            }
        };

        return_string
    }

    pub fn create_dockerfile(
        &self,
        validator_type: &ValidatorType,
        docker_path: PathBuf,
        content: Option<&str>,
        index: Option<i32>,
    ) -> Result<PathBuf, Box<dyn std::error::Error>> {
        if !(validator_type != &ValidatorType::Bootstrap
            || validator_type != &ValidatorType::Standard
            || validator_type != &ValidatorType::NonVoting)
        {
            return Err(boxed_error!(format!(
                "Invalid validator type: {}. Exiting...",
                validator_type
            )));
        }

        if docker_path.exists() {
            fs::remove_dir_all(&docker_path)?;
        }
        fs::create_dir_all(&docker_path)?;

        let solana_build_directory = if self.deploy_method == "tar" {
            "solana-release"
        } else {
            "farf"
        };

        //TODO: implement SKIP
        let dockerfile = format!(
            r#"
FROM {}
RUN apt-get update
RUN apt-get install -y iputils-ping curl vim bzip2

RUN useradd -ms /bin/bash solana
RUN adduser solana sudo
USER solana

COPY ./fetch-perf-libs.sh ./fetch-spl.sh ./scripts /home/solana/
COPY ./net ./multinode-demo /home/solana/

RUN mkdir -p /home/solana/k8s-cluster-scripts
COPY ./k8s-cluster/src/scripts /home/solana/k8s-cluster-scripts

RUN mkdir -p /home/solana/ledger
COPY --chown=solana:solana ./config-k8s/bootstrap-validator  /home/solana/ledger

RUN mkdir -p /home/solana/.cargo/bin

COPY ./{solana_build_directory}/bin/ /home/solana/.cargo/bin/
COPY ./{solana_build_directory}/version.yml /home/solana/

RUN mkdir -p /home/solana/config
ENV PATH="/home/solana/.cargo/bin:${{PATH}}"

WORKDIR /home/solana
"#,
            self.image_config.base_image
        );

        let dockerfile = format!(
            "{}\n{}",
            dockerfile,
            self.insert_client_accounts_if_present(index)
        );

        debug!("dockerfile: {}", dockerfile);
        std::fs::write(
            docker_path.as_path().join("Dockerfile"),
            content.unwrap_or(dockerfile.as_str()),
        )
        .expect("saved Dockerfile");
        Ok(docker_path)
    }

    fn push_image(image: String, identifier: &str) -> Result<(), Box<dyn Error + Send>> {
        let progress_bar = new_spinner_progress_bar();
        progress_bar.set_message(format!(
            "{ROCKET}Pushing {} image to registry...",
            identifier
        ));
        let command = format!("docker push '{}'", image);
        let output = Command::new("sh")
            .arg("-c")
            .arg(&command)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("Failed to execute command")
            .wait_with_output()
            .expect("Failed to push image");

        if !output.status.success() {
            return Err(boxed_error!(output.status.to_string()));
        }
        progress_bar.finish_and_clear();
        Ok(())
    }

    pub fn push_validator_image(
        &self,
        validator_type: &ValidatorType,
    ) -> Result<(), Box<dyn Error + Send>> {
        info!("Pushing {} image...", validator_type);
        let image = format!(
            "{}/{}-{}:{}",
            self.image_config.registry,
            validator_type,
            self.image_config.image_name,
            self.image_config.tag
        );

        Self::push_image(image, format!("{}", validator_type).as_str())
    }

    pub fn push_client_images(&self, num_clients: i32) -> Result<(), Box<dyn Error + Send>> {
        info!("Pushing client images...");
        (0..num_clients).into_par_iter().try_for_each(|i| {
            let image = format!(
                "{}/{}-{}-{}:{}",
                self.image_config.registry,
                ValidatorType::Client,
                "image",
                i,
                self.image_config.tag
            );

            Self::push_image(image, format!("client-{}", i).as_str())
        })
    }
}
