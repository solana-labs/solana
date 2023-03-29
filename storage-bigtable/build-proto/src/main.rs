fn main() -> Result<(), std::io::Error> {
    const PROTOC_ENVAR: &str = "PROTOC";
    if std::env::var(PROTOC_ENVAR).is_err() {
        #[cfg(not(windows))]
        std::env::set_var(PROTOC_ENVAR, protobuf_src::protoc());
    }

    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    let out_dir = manifest_dir.join("../proto");
    let googleapis = manifest_dir.join("googleapis");

    println!("Google API directory: {}", googleapis.display());
    println!("output directory: {}", out_dir.display());

    tonic_build::configure()
        .build_client(true)
        .build_server(false)
        .out_dir(&out_dir)
        .compile(
            &[googleapis.join("google/bigtable/v2/bigtable.proto")],
            &[googleapis],
        )
}
