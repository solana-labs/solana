extern crate cc;

fn main() {
    cc::Build::new()
        .file("cpu-crypt/chacha20_core.c")
        .file("cpu-crypt/chacha_cbc.c")
        .compile("libcpu-crypt");
}
