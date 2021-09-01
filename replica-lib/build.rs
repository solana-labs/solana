fn main() -> Result<(), Box<dyn std::error::Error>> {
    // compiling protos using path on build time
    tonic_build::configure().compile(&["proto/accountsdb_repl.proto"], &["proto"])?;
    Ok(())
}
