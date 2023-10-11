use {
    git2::{IndexAddOption, Repository},
    serde::{Deserialize, Serialize},
    std::{
        fs::{self, create_dir_all},
        io::ErrorKind,
        path::PathBuf,
        process::Command,
    },
};

#[derive(Debug, Default, Deserialize, Serialize)]
struct RegistryConfig {
    dl: String,
    api: Option<String>,
}

pub struct DummyGitIndex {}

impl DummyGitIndex {
    pub fn create_or_update_git_repo(root_dir: PathBuf, server_url: &str) {
        create_dir_all(&root_dir).expect("Failed to create root directory");

        let expected_config = serde_json::to_string(&RegistryConfig {
            dl: format!(
                "{}/api/v1/crates/{{crate}}/{{version}}/download",
                server_url
            ),
            api: Some(server_url.to_string()),
        })
        .expect("Failed to create expected config");

        let config_path = root_dir.join("config.json");
        let config_written = if let Ok(config) = fs::read_to_string(&config_path) {
            if config != expected_config {
                fs::write(config_path, expected_config).expect("Failed to update config");
                true
            } else {
                false
            }
        } else {
            fs::write(config_path, expected_config).expect("Failed to write config");
            true
        };

        #[cfg(unix)]
        use std::os::unix::fs::symlink;
        #[cfg(windows)]
        use std::os::windows::fs::symlink_dir as symlink;

        let new_symlink = match symlink(".", root_dir.join("index")) {
            Ok(()) => true,
            Err(ref err) if err.kind() == ErrorKind::AlreadyExists => false,
            Err(err) => panic!("Failed to create a symlink: {}", err),
        };

        let new_git_symlink = match symlink(".git", root_dir.join("git")) {
            Ok(()) => true,
            Err(ref err) if err.kind() == ErrorKind::AlreadyExists => false,
            Err(err) => panic!("Failed to create git symlink: {}", err),
        };

        let repository = Repository::init(&root_dir).expect("Failed to GIT init");

        let empty = repository
            .is_empty()
            .expect("Failed to check if GIT repo is empty");

        if empty || config_written || new_symlink || new_git_symlink {
            let mut index = repository.index().expect("cannot get the Index file");
            index
                .add_all(
                    ["config.json", "index"].iter(),
                    IndexAddOption::DEFAULT,
                    None,
                )
                .expect("Failed to add modified files to git index");
            index.write().expect("Failed to update the git index");

            let tree = index
                .write_tree()
                .and_then(|tree_id| repository.find_tree(tree_id))
                .expect("Failed to get tree");

            let signature = repository.signature().expect("Failed to get signature");

            if empty {
                repository.commit(
                    Some("HEAD"),
                    &signature,
                    &signature,
                    "Created new repo",
                    &tree,
                    &[],
                )
            } else {
                let oid = repository
                    .refname_to_id("HEAD")
                    .expect("Failed to get HEAD ref");
                let parent = repository
                    .find_commit(oid)
                    .expect("Failed to find parent commit");

                repository.commit(
                    Some("HEAD"),
                    &signature,
                    &signature,
                    "Updated GIT repo",
                    &tree,
                    &[&parent],
                )
            }
            .expect("Failed to commit the changes");
        }

        Command::new("git")
            .current_dir(&root_dir)
            .arg("update-server-info")
            .status()
            .expect("git update-server-info failed");
    }
}
