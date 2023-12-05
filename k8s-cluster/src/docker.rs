use {
    crate::{boxed_error, initialize_globals, ValidatorType, SOLANA_ROOT},
    log::*,
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
        match self.create_base_image(validator_type) {
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

    pub fn create_base_image(
        &self,
        validator_type: &ValidatorType,
    ) -> Result<Output, Box<dyn Error>> {
        let image_name = format!("{}-{}", validator_type, self.image_config.image_name);
        let docker_path = SOLANA_ROOT.join(format!("{}/{}", "docker-build", validator_type));

        let dockerfile_path = match self.create_dockerfile(validator_type, docker_path, None) {
            Ok(res) => res,
            Err(err) => return Err(err),
        };

        info!("Tmp: {}", dockerfile_path.as_path().display());
        info!("Exists: {}", dockerfile_path.as_path().exists());

        // We use std::process::Command here because Docker-rs is very slow building dockerfiles
        // when they are in large repos. Docker-rs doesn't seem to support the `--file` flag natively.
        // so we result to using std::process::Command
        let dockerfile = dockerfile_path.join("Dockerfile");
        let context_path = SOLANA_ROOT.display().to_string();
        let command = format!(
            "docker build -t {}/{}:{} -f {:?} {}",
            self.image_config.registry, image_name, self.image_config.tag, dockerfile, context_path
        );
        match Command::new("sh")
            .arg("-c")
            .arg(&command)
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("Failed to execute command")
            .wait_with_output()
        {
            Ok(res) => Ok(res),
            Err(err) => Err(Box::new(err)),
        }
    }

    fn insert_client_accounts_if_present(&self) -> String {
        if SOLANA_ROOT.join("config-k8s/client-accounts.yml").exists() {
            return r#"
COPY --chown=solana:solana ./config-k8s/client-accounts.yml /home/solana
            "#
            .to_string();
        }
        "".to_string()
    }

    pub fn create_dockerfile(
        &self,
        validator_type: &ValidatorType,
        docker_path: PathBuf,
        content: Option<&str>,
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
            self.insert_client_accounts_if_present()
        );

        info!("dockerfile: {}", dockerfile);
        std::fs::write(
            docker_path.as_path().join("Dockerfile"),
            content.unwrap_or(dockerfile.as_str()),
        )
        .expect("saved Dockerfile");
        Ok(docker_path)
    }

    pub fn push_image(&self, validator_type: &ValidatorType) -> Result<(), Box<dyn Error>> {
        let image = format!(
            "{}/{}-{}:{}",
            self.image_config.registry,
            validator_type,
            self.image_config.image_name,
            self.image_config.tag
        );

        let command = format!("docker push '{}'", image);
        let output = Command::new("sh")
            .arg("-c")
            .arg(&command)
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("Failed to execute command")
            .wait_with_output()
            .expect("Failed to push image");

        if !output.status.success() {
            return Err(boxed_error!(output.status.to_string()));
        }
        Ok(())
    }
}
