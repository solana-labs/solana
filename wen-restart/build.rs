extern crate rustc_version;

use std::io::Result;

fn main() -> Result<()> {
    const PROTOC_ENVAR: &str = "PROTOC";
    if std::env::var(PROTOC_ENVAR).is_err() {
        #[cfg(not(windows))]
        std::env::set_var(PROTOC_ENVAR, protobuf_src::protoc());
    }

    let proto_base_path = std::path::PathBuf::from("proto");
    let proto_files = ["wen_restart.proto"];
    let mut protos = Vec::new();
    for proto_file in &proto_files {
        let proto = proto_base_path.join(proto_file);
        println!("cargo:rerun-if-changed={}", proto.display());
        protos.push(proto);
    }
    // Generate rust files from protos.
    prost_build::compile_protos(&protos, &[proto_base_path])?;
    Ok(())
}
