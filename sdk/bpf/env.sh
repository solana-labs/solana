#
# Configures the BPF SDK environment
#

if [ -z "$bpf_sdk" ]; then
  bpf_sdk=.
fi

# Ensure the sdk is installed
"$bpf_sdk"/scripts/install.sh

# Use the SDK's version of llvm to build the compiler-builtins for BPF
export CC="$bpf_sdk/dependencies/llvm-native/bin/clang"
export AR="$bpf_sdk/dependencies/llvm-native/bin/llvm-ar"
export OBJDUMP="$bpf_sdk/dependencies/llvm-native/bin/llvm-objdump"
export OBJCOPY="$bpf_sdk/dependencies/llvm-native/bin/llvm-objcopy"

# Use the SDK's version of Rust to build for BPF
export RUSTUP_TOOLCHAIN=bpf
export RUSTFLAGS="
    -C lto=no \
    -C opt-level=2 \
    -C link-arg=-z -C link-arg=notext \
    -C link-arg=-T$bpf_sdk/rust/bpf.ld \
    -C link-arg=--Bdynamic \
    -C link-arg=-shared \
    -C link-arg=--entry=entrypoint \
    -C link-arg=-no-threads \
    -C linker=$bpf_sdk/dependencies/llvm-native/bin/ld.lld"

# CARGO may be set if run from within cargo, causing
# incompatibilities between cargo and xargo versions
unset CARGO

export XARGO="$bpf_sdk"/dependencies/bin/xargo
export XARGO_TARGET=bpfel-unknown-unknown
export XARGO_HOME="$bpf_sdk/dependencies/xargo"
export XARGO_RUST_SRC="$bpf_sdk/dependencies/rust-bpf-sysroot/src"
export RUST_COMPILER_RT_ROOT="$bpf_sdk/dependencies/rust-bpf-sysroot/src/compiler-rt"
