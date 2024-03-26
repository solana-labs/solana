extern crate rustc_version;

use std::io::Result;

fn main() -> Result<()> {
    const PROTOC_ENVAR: &str = "PROTOC";
    if std::env::var(PROTOC_ENVAR).is_err() {
        #[cfg(not(windows))]
        std::env::set_var(PROTOC_ENVAR, protobuf_src::protoc());
    }

    let proto_base_path = std::path::PathBuf::from("proto");
    let proto = proto_base_path.join("wen_restart.proto");
    println!("cargo:rerun-if-changed={}", proto.display());

    // Generate rust files from protos.
    prost_build::compile_protos(&[proto], &[proto_base_path])?;
    Ok(())
}
