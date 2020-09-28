use std::path::PathBuf;

fn main() {
    let src = PathBuf::from("./src");
    let includes = &[src.clone()];

    // Generate BTreeMap fields for all messages. This forces encoded output to be consistent, so
    // that encode/decode roundtrips can use encoded output for comparison. Otherwise trying to
    // compare based on the Rust PartialEq implementations is difficult, due to presence of NaN
    // values.
    let mut config = prost_build::Config::new();
    config.btree_map(&["."]);
    config.out_dir(&src);
    config
        .compile_protos(&[src.join("confirmed_block.proto")], includes)
        .unwrap();
}
