fn main() {
    let proto_base_path = std::path::PathBuf::from("proto");
    let protos = ["context.proto", "invoke.proto", "sysvar.proto", "txn.proto"];
    let protos_path: Vec<_> = protos
        .iter()
        .map(|name| proto_base_path.join(name))
        .collect();

    protos_path
        .iter()
        .for_each(|proto| println!("cargo:rerun-if-changed={}", proto.display()));

    prost_build::compile_protos(protos_path.as_ref(), &[proto_base_path])
        .expect("Failed to compile protobuf files");
}
