use {
    crate::{boxed_error, initialize_globals, SOLANA_ROOT},
    docker_api::{self, Docker, opts},
    log::*,
    std::{
        env,
        path::PathBuf,
        error::Error,
        fs,
        process::{Command, Stdio, Output},
    },
};

pub const DEFAULT_IMAGE: &str = "ubuntu:20.04";
const URI_ENV_VAR: &str = "unix:///var/run/docker.sock";

#[derive(Clone, Debug)]
pub struct DockerImageConfig<'a> {
    pub base_image: &'a str,
    pub tag: &'a str,
}

pub struct DockerConfig<'a> {
    image_config: DockerImageConfig<'a>,
    deploy_method: &'a str,
}

impl<'a> DockerConfig<'a> {
    pub fn new(
        image_config: DockerImageConfig<'a>, 
        deploy_method: &'a str
    ) -> Self {
        initialize_globals();
        DockerConfig {
            image_config,
            deploy_method,
        }
    }

    pub fn init_runtime(&self) -> Docker {
        let _ = env_logger::try_init();
        if let Ok(uri) = env::var(URI_ENV_VAR) {
            Docker::new(uri).unwrap()
        } else {
            #[cfg(unix)]
            {
                let uid = nix::unistd::Uid::effective();
                let docker_dir = PathBuf::from(format!("/run/user/{uid}/docker"));
                let docker_root_dir = PathBuf::from("/var/run");
                if docker_dir.exists() {
                    Docker::unix(docker_dir.join("docker.sock"))
                } else if docker_root_dir.exists() {
                    Docker::unix(docker_root_dir.join("docker.sock"))
                } else {
                    panic!(
                        "Docker socket not found. Tried {URI_ENV_VAR} env variable, {} and {}",
                        docker_dir.display(),
                        docker_root_dir.display()
                    );
                }
            }
            #[cfg(not(unix))]
            {
                panic!("Docker socket not found. Try setting the {URI_ENV_VAR} env variable",);
            }
        }
    }

    pub async fn build_image(&self, validator_type: &str) -> Result<(), Box<dyn Error>> {
        let docker = self.init_runtime();
        match self.create_base_image(&docker, validator_type).await {
            Ok(res) => {
                if res.status.success() {
                    info!("Successfully created base Image");
                    return Ok(());
                } else {
                    error!("Failed to build base image");
                    return Err(boxed_error!(String::from_utf8_lossy(&res.stderr)));
                }
            },
            Err(err) => return Err(err),
        };
    }

    pub async fn create_base_image(
        &self,
        docker: &Docker,
        validator_type: &str,
    ) -> Result<Output, Box<dyn Error>> {
        let tag = format!("{}-{}", validator_type, self.image_config.tag);

        let images = docker.images();
        let _ = images
            .get(tag.as_str())
            .remove(
                &opts::ImageRemoveOpts::builder()
                    .force(true)
                    .noprune(true)
                    .build(),
            )
            .await;
    
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
        let command = format!("docker build -t {} -f {:?} {}", tag, dockerfile, context_path);
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

    pub fn create_dockerfile(
        &self,
        validator_type: &str,
        docker_path: PathBuf,
        content: Option<&str>
    ) -> Result<PathBuf, Box<dyn std::error::Error>>  {
        if !(validator_type != "validator" || validator_type != "bootstrap") {
            return Err(boxed_error!(format!("Invalid validator type: {}. Exiting...", validator_type)));
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
        
        // SOLANA_ROOT.
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
RUN mkdir -p /home/solana/.cargo/bin

COPY ./{solana_build_directory}/bin/* /home/solana/.cargo/bin/
COPY ./{solana_build_directory}/version.yml /home/solana/

RUN mkdir -p /home/solana/config

WORKDIR /home/solana
"#,
        self.image_config.base_image);

        info!("dockerfile: {}", dockerfile);

        std::fs::write(
            docker_path.as_path().join("Dockerfile"),
            content.unwrap_or(dockerfile.as_str()),
        )
        .expect("saved Dockerfile");
        Ok(docker_path)
    }
    
}

// RUN apt install -y iputils-ping curl vim bzip2 psmisc \
// iproute2 software-properties-common apt-transport-https \
// ca-certificates openssh-client openssh-server