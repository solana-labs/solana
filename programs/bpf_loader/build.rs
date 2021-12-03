use {
    regex::Regex,
    std::{
        fs::File,
        io::{prelude::*, BufWriter, Read},
        path::PathBuf,
        process::exit,
        str,
    },
};

/**
 * Extract a list of registered syscall names and save it in a file
 * for distribution with the SDK.  This file is read by cargo-build-bpf
 * to verify undefined symbols in a .so module that cargo-build-bpf has built.
 */
fn main() {
    let path = PathBuf::from("src/syscalls.rs");
    let mut file = match File::open(&path) {
        Ok(x) => x,
        _ => exit(1),
    };
    let mut text = vec![];
    file.read_to_end(&mut text).unwrap();
    let text = str::from_utf8(&text).unwrap();
    let path = PathBuf::from("../../sdk/bpf/syscalls.txt");
    let file = match File::create(&path) {
        Ok(x) => x,
        _ => exit(1),
    };
    let mut out = BufWriter::new(file);
    let sysc_re = Regex::new(r#"register_syscall_by_name\([[:space:]]*b"([^"]+)","#).unwrap();
    for caps in sysc_re.captures_iter(text) {
        let name = caps[1].to_string();
        writeln!(out, "{}", name).unwrap();
    }
}
