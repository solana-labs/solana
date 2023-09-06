use {
    crate::{
        boxed_error, initialize_globals, load_env_variable_by_name, new_spinner_progress_bar,
        DOCKER_WHALE, SOLANA_ROOT,
    },
    docker_api::{self, opts, Docker},
    log::*,
    std::{
        env,
        error::Error,
        fs,
        path::PathBuf,
        process::{Command, Output, Stdio},
    },
};

const URI_ENV_VAR: &str = "unix:///var/run/docker.sock";

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
    docker: Docker,
    registry_username: Option<String>,
    registry_password: Option<String>,
}

fn init_runtime() -> Docker {
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

impl<'a> DockerConfig<'a> {
    pub fn new(image_config: DockerImageConfig<'a>, deploy_method: &'a str) -> Self {
        initialize_globals();
        DockerConfig {
            image_config,
            deploy_method,
            docker: init_runtime(),
            registry_username: match load_env_variable_by_name("REGISTRY_USERNAME") {
                Ok(username) => Some(username),
                Err(_) => None,
            },
            registry_password: match load_env_variable_by_name("REGISTRY_PASSWORD") {
                Ok(password) => Some(password),
                Err(_) => None,
            },
        }
    }

    pub fn registry_credentials_set(&self) -> bool {
        if self.registry_username.is_none() || self.registry_username.is_none() {
            return false;
        }
        true
    }

    pub async fn build_image(&self, validator_type: &str) -> Result<(), Box<dyn Error>> {
        match self.create_base_image(validator_type).await {
            Ok(res) => {
                if res.status.success() {
                    info!("Successfully created base Image");
                    return Ok(());
                } else {
                    error!("Failed to build base image");
                    return Err(boxed_error!(String::from_utf8_lossy(&res.stderr)));
                }
            }
            Err(err) => return Err(err),
        };
    }

    pub async fn create_base_image(&self, validator_type: &str) -> Result<Output, Box<dyn Error>> {
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

    pub fn create_dockerfile(
        &self,
        validator_type: &str,
        docker_path: PathBuf,
        content: Option<&str>,
    ) -> Result<PathBuf, Box<dyn std::error::Error>> {
        if !(validator_type != "validator" || validator_type != "bootstrap") {
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
ENV PATH="/home/solana/.cargo/bin:${{PATH}}"

WORKDIR /home/solana
"#,
            self.image_config.base_image
        );

        info!("dockerfile: {}", dockerfile);

        std::fs::write(
            docker_path.as_path().join("Dockerfile"),
            content.unwrap_or(dockerfile.as_str()),
        )
        .expect("saved Dockerfile");
        Ok(docker_path)
    }

    pub async fn push_image(&self, validator_type: &str) -> Result<(), Box<dyn Error>> {
        let username = match &self.registry_username {
            Some(username) => username,
            None => {
                return Err(boxed_error!(
                    "No username set for registry! Is REGISTRY_USERNAME set?"
                ))
            }
        };
        let password = match &self.registry_password {
            Some(password) => password,
            None => {
                return Err(boxed_error!(
                    "No password set for registry! Is REGISTRY_PASSWORD set?"
                ))
            }
        };

        // self.docker
        let image = format!(
            "{}/{}-{}",
            self.image_config.registry, validator_type, self.image_config.image_name
        );
        let auth = opts::RegistryAuth::Password {
            username: password.to_string(),
            password: username.to_string(),
            email: None,
            server_address: None,
        };

        let options = opts::ImagePushOpts::builder()
            .tag(self.image_config.tag)
            .auth(auth)
            .build();
        let progress_bar = new_spinner_progress_bar();
        progress_bar.set_message(format!("{DOCKER_WHALE}Pushing image {} to registry", image));

        match self.docker.images().push(image, &options).await {
            Ok(res) => Ok(res),
            Err(err) => Err(boxed_error!(format!("{}", err))),
        }
    }
}

// RUN apt install -y iputils-ping curl vim bzip2 psmisc \
// iproute2 software-properties-common apt-transport-https \
// ca-certificates openssh-client openssh-server
