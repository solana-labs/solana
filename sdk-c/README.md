# Solana SDK-C

This crate exposes a C API to Solana functions written in Rust. The crate generates a static library, and uses `cbindgen`
to generate a header file during the build. To generate both:

```shell
$ cd <path/to/solana/repo>/sdk-c
$ SOLANA_H_OUT_DIR="$(pwd)/include" cargo build
```

This will generate the static library in `<path/to/solana/repo>/target/deps` and the header file in
`<path/to/solana/repo>/sdk-c/include`.