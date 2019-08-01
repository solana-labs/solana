use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    let crate_dir = env::var("CARGO_MANIFEST_DIR").unwrap();

    let out_path = if let Ok(path) = env::var("SOLANA_H_OUT_DIR") {
        PathBuf::from(path)
    } else {
        let out_dir = env::var("OUT_DIR").unwrap();
        let out_dir = Path::new(&out_dir);
        out_dir.join(Path::new("include"))
    };

    // Ensure `out_path` exists
    fs::create_dir_all(&out_path).unwrap_or_else(|err| {
        if err.kind() != std::io::ErrorKind::AlreadyExists {
            panic!("Unable to create {:#?}: {:?}", out_path, err);
        }
    });

    let out_path = out_path.join(Path::new("solana.h"));
    let out_path = out_path.to_str().unwrap();

    cbindgen::generate(crate_dir)
        .unwrap()
        .write_to_file(out_path);
}
