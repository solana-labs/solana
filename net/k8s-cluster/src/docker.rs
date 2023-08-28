use {
    crate::{boxed_error, initialize_globals, SOLANA_ROOT},
    docker_api::{self, api, Docker, opts, models::ImageBuildChunk, Images},
    fs_extra,
    futures_util::{StreamExt, TryStreamExt},
    log::*,
    std::{
        env,
        path::{Path, PathBuf},
        error::Error,
        fs,
        process::{Command, Stdio, Output},
    },
    tempfile::TempDir,
};

pub const DEFAULT_IMAGE: &str = "ubuntu:20.04";
pub const DEFAULT_CMD: &str = "sleep inf";
pub const DEFAULT_CMD_ARRAY: &[&str] = &["sleep", "inf"];
pub const TEST_IMAGE_PATH: &str = "/var/test123";
const URI_ENV_VAR: &str = "unix:///var/run/docker.sock";

pub struct DockerConfig<'a> {
    deploy_method: &'a str,
}

impl<'a> DockerConfig<'a> {
    pub fn new(deploy_method: &'a str) -> Self {
        initialize_globals();
        DockerConfig {
            deploy_method: deploy_method,
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

    pub async fn create_base_container(
        &self,
        docker: &Docker,
        name: &str,
        opts: Option<opts::ContainerCreateOpts>,
    ) -> api::Container {
        match self.cleanup_container(docker, name).await {
            Ok(val) => info!("val: {}", val),
            Err(err) => error!("err: {}", err),
        };
    
        let opts = opts.unwrap_or_else(|| {
            opts::ContainerCreateOpts::builder()
                .image(DEFAULT_IMAGE)
                .name(name)
                .command(DEFAULT_CMD_ARRAY)
                .build()
        });
        let d = docker
            .containers()
            .create(&opts)
            .await
            .expect("created base container");
        let k = d.inspect().await;
        docker.containers().get(name)
    }

    pub async fn cleanup_container(&self, docker: &Docker, name: &str) -> Result<String, Box<dyn Error>> {
        let res = docker
            .containers()
            .get(name)
            .remove(&opts::ContainerRemoveOpts::builder().force(true).build())
            .await;
        match res {
            Ok(val) => Ok(val),
            Err(err) => Err(boxed_error!(err)),
        }

    }

    pub async fn container_create_inspect_remove(&self) {
        let docker = self.init_runtime();
        let container_name = "greg";
    
        let container = self.create_base_container(&docker, container_name, None).await;
        // container.
        let inspect_result = container.inspect().await;
        if inspect_result.is_ok() {
            info!("created container!");
        }
        // inspect_result.o
        // assert!(inspect_result.is_ok());
    
        // let remove_result = container.delete().await;
        // assert!(remove_result.is_ok());
    
        // let inspect_result = container.inspect().await;
        // assert!(inspect_result.is_err());
    }

    pub fn setup_build_directory(
        &self,
        source_directory: PathBuf,
        build_directory: PathBuf,
    )  -> Result<(), Box<dyn Error>> {
        // if build_directory.exists() {
        //     fs::remove_dir_all(&build_directory)?;
        // }
        // fs::create_dir_all(&build_directory)?;

        let solana_build_directory = if self.deploy_method == "tar" {
            "solana-release"
        } else {
            "farf"
        };

        let bin_dir = format!("{solana_build_directory}/bin");
        let version = format!("{solana_build_directory}/version.yml");
        
        // List of files and folders to copy
        let docs_to_copy: Vec<PathBuf> = vec![
            SOLANA_ROOT.join("fetch-perf-libs.sh"),
            SOLANA_ROOT.join("fetch-spl.sh"),
            SOLANA_ROOT.join("scripts"),
            SOLANA_ROOT.join("net"),
            SOLANA_ROOT.join("multinode-demo"),
            SOLANA_ROOT.join(bin_dir.as_str()),
            SOLANA_ROOT.join(version.as_str()),
        ];

        let options = fs_extra::dir::CopyOptions::new(); //Initialize default values for CopyOptions
        fs_extra::copy_items(&docs_to_copy, build_directory, &options)?;


         // Loop through the items and copy them
        // for item in docs_to_copy {
        //     let source_path = source_directory.join(item);
        //     let destination_path = build_directory.join(item);

        //     if source_path.is_dir() {
        //         fs::copy(&source_path, &destination_path)?;
        //     } else if source_path.is_file() {
        //         fs::copy(&source_path, &destination_path)?;
        //     }
        // }

        Ok(())


        
    }

    pub async fn image_create_inspect_delete(&self) -> Result<(), Box<dyn Error>> {
        let docker = self.init_runtime();

        // self.setup_build_directory(SOLANA_ROOT.join(""), SOLANA_ROOT.join("dockerfile-build"))?;
        // self.setup_build_directory(SOLANA_ROOT.join(""), SOLANA_ROOT.join("tmp-dir-docker/bootstrap"))?;

        

        match self.create_base_image(&docker, "test-greg", None).await {
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
    
        // let image = self.create_base_image(&docker, "test-greg", None).await;
        // assert!(image.inspect().await.is_ok());
        // let delete_res = image
        //     .remove(
        //         &opts::ImageRemoveOpts::builder()
        //             .force(true)
        //             .noprune(true)
        //             .build(),
        //     )
        //     .await;
        // info!("delete res: {delete_res:#?}");
        // assert!(delete_res.is_ok());
        // assert!(image.inspect().await.is_err());
        Ok(())
    }

    // #[cfg(feature = "par-compress")]
    pub async fn create_base_image(
        &self,
        docker: &Docker,
        tag: &str,
        opts: Option<opts::ImageBuildOpts>,
    // ) -> Result<api::Image, Box<dyn Error>> {
    ) -> Result<Output, Box<dyn Error>> {

        let images = docker.images();
        let _ = images
            .get(tag)
            .remove(
                &opts::ImageRemoveOpts::builder()
                    .force(true)
                    .noprune(true)
                    .build(),
            )
            .await;
    
        let dockerfile_path = match self.tempdir_build_dockerfile("bootstrap", None) {
            Ok(res) => res,
            Err(err) => return Err(err),
        };

        info!("Tmp: {}", dockerfile_path.as_path().display());
        info!("Exists: {}", dockerfile_path.as_path().exists());

        // match self.setup_build_directory(SOLANA_ROOT.join(""), dockerfile_path) {
        //     Ok(res) => info!("setup success!"),
        //     Err(err) => error!("err: {}", err),
        // };

        let image_name = "greg-test";
        let dockerfile = dockerfile_path.join("Dockerfile");
        let context_path = SOLANA_ROOT.display().to_string();
        let command = format!("docker build -t {} -f {:?} {}", image_name, dockerfile, context_path);
        let output = Command::new("sh")
            .arg("-c")
            .arg(&command)
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("Failed to execute command")
            .wait_with_output();

        match output {
            Ok(res) => Ok(res),
            Err(err) => Err(Box::new(err)),
    
        }
        


            
        // let stdout = output.stdout.take().expect("Failed to get stdout handle");
        // let stderr = output.stderr.take().expect("Failed to get stderr handle");



        // if output.status.success() {
        //     let stdout = String::from_utf8_lossy(&output.stdout);
        //     println!("Command output:\n{}", stdout);
        // } else {
        //     let stderr = String::from_utf8_lossy(&output.stderr);
        //     println!("Command failed with error:\n{}", stderr);
        // }

        // Ok(())
        

        //copy up a dir
        // fs::copy(dockerfile_path.join("Dockerfile"), SOLANA_ROOT.join("Dockerfile"))?;
        // fs::copy(dockerfile_path.join("Dockerfile"), SOLANA_ROOT.join("dockerfile-build/Dockerfile"))?;



        // println!("Tmp: {}", dockerfile_path.as_path().display());
        // println!("Exists: {}", dockerfile_path.as_path().exists());
        // let solana_root = SOLANA_ROOT.join("");

        
        // info!("dockerfile-build-path: {:?}", SOLANA_ROOT.join("tmp-dir-docker/bootstrap"));
        // let opts = opts.unwrap_or_else(|| opts::ImageBuildOpts::builder(SOLANA_ROOT.join("tmp-dir-docker/bootstrap")).tag(tag).build());
        // info!("here");

        // // TODO this is super slow when we try to build Dockerfile from main solana repo
        // // Sooo. let's create a temp directory, copy everything into it that we need. and then copy 
        // // the Dockerfile into that folder. And then we build from there.
        // let mut image_stream = images.build(&opts);
        // let mut digest = None;
        // info!("suh");
        // while let Some(chunk) = image_stream.next().await {
        //     println!("{chunk:?}");
        //     assert!(chunk.is_ok());
        //     if matches!(chunk, Ok(ImageBuildChunk::Digest { .. })) {
        //         digest = Some(chunk);
        //     }
        // }
    
        // match digest.unwrap().unwrap() {
        //     ImageBuildChunk::Digest { aux } => Ok(docker.images().get(aux.id)),
        //     chunk => panic!("invalid chunk {chunk:?}"),
        // }
    }

    // TODO maybe don't save this in some random temp dir in case we want to access 
    // for debug purposes
    pub fn tempdir_build_dockerfile(&self, validator_type: &str, content: Option<&str>) -> Result<PathBuf, Box<dyn std::error::Error>>  {
        if !(validator_type != "validator" || validator_type != "bootstrap") {
            return Err(boxed_error!("Invalid validator type. Exiting..."));
        }
        let docker_path = format!("{}/{}", "docker-build", validator_type);

        let temp_release_dir = SOLANA_ROOT.join(docker_path);
        if temp_release_dir.exists() {
            fs::remove_dir_all(&temp_release_dir)?;
        }
        fs::create_dir_all(&temp_release_dir)?;

        


        let solana_build_directory = if self.deploy_method == "tar" {
            "solana-release"
        } else {
            "farf"
        };

        // let default_dockerfile = format!(
        //     "FROM {DEFAULT_IMAGE}\nRUN echo 1234 > {TEST_IMAGE_PATH}\nRUN echo 321\nCMD sleep inf",
        // );
        
        let solana_root_str = SOLANA_ROOT.display().to_string();
        // SOLANA_ROOT.
        //TODO: implement SKIP
        let dockerfile = format!(
r#" 
FROM {DEFAULT_IMAGE}             
RUN apt update
RUN apt install -y iputils-ping curl vim bzip2 

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
"#
        );

        info!("dockerfile: {}", dockerfile);

        std::fs::write(
            temp_release_dir.as_path().join("Dockerfile"),
            content.unwrap_or(dockerfile.as_str()),
        )
        .expect("saved Dockerfile");
        Ok(temp_release_dir)
    }
    
}

// RUN apt install -y iputils-ping curl vim bzip2 psmisc \
// iproute2 software-properties-common apt-transport-https \
// ca-certificates openssh-client openssh-server

// COPY {solana_root_str}/fetch-perf-libs.sh {solana_root_str}/fetch-spl.sh {solana_root_str}/scripts /home/solana/
// COPY {solana_root_str}/net {solana_root_str}/multinode-demo /home/solana/

// COPY {solana_root_str}/{solana_build_directory}/bin/* /home/solana/.cargo/bin/
// COPY {solana_root_str}/{solana_build_directory}/version.yaml /home/solana/