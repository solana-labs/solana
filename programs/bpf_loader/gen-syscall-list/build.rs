use {
    regex::Regex,
    std::{
        fs::File,
        io::{prelude::*, BufWriter, Read},
        path::PathBuf,
        str,
    },
};

/**
 * Extract a list of registered syscall names and save it in a file
 * for distribution with the SDK.  This file is read by cargo-build-sbf
 * to verify undefined symbols in a .so module that cargo-build-sbf has built.
 */
fn main() {
    let syscalls_rs_path = PathBuf::from("../src/syscalls/mod.rs");
    let syscalls_txt_path = PathBuf::from("../../../sdk/sbf/syscalls.txt");
    println!(
        "cargo:warning=(not a warning) Generating {1} from {0}",
        syscalls_rs_path.display(),
        syscalls_txt_path.display()
    );

    let mut file = match File::open(&syscalls_rs_path) {
        Ok(x) => x,
        Err(err) => panic!("Failed to open {}: {}", syscalls_rs_path.display(), err),
    };
    let mut text = vec![];
    file.read_to_end(&mut text).unwrap();
    let text = str::from_utf8(&text).unwrap();
    let file = match File::create(&syscalls_txt_path) {
        Ok(x) => x,
        Err(err) => panic!("Failed to create {}: {}", syscalls_txt_path.display(), err),
    };
    let mut out = BufWriter::new(file);
    let sysc_re = Regex::new(r#"register_syscall_by_name\([[:space:]]*b"([^"]+)","#).unwrap();
    for caps in sysc_re.captures_iter(text) {
        let name = caps[1].to_string();
        writeln!(out, "{name}").unwrap();
    }
}
