fn main() -> Result<(), std::io::Error> {
    let proto_base_path = std::path::PathBuf::from("proto");
    let proto_files = ["confirmed_block.proto", "transaction_by_addr.proto"];
    let mut protos = Vec::new();
    for proto_file in &proto_files {
        let proto = proto_base_path.join(proto_file);
        println!("cargo::rerun-if-changed={}", proto.display());
        protos.push(proto);
    }

    tonic_build::configure()
        .build_client(true)
        .build_server(false)
        .format(true)
        .type_attribute(
            "TransactionErrorType",
            "#[cfg_attr(test, derive(enum_iterator::IntoEnumIterator))]",
        )
        .type_attribute(
            "InstructionErrorType",
            "#[cfg_attr(test, derive(enum_iterator::IntoEnumIterator))]",
        )
        .compile(&protos, &[proto_base_path])
}
