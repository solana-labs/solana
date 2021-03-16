#
# Configures the BPF SDK environment
#

if [ -z "$bpf_sdk" ]; then
  bpf_sdk=.
fi

# Ensure the sdk is installed
"$bpf_sdk"/scripts/install.sh

# Use the SDK's version of llvm to build the compiler-builtins for BPF
export CC="$bpf_sdk/dependencies/bpf-tools/llvm/bin/clang"
export AR="$bpf_sdk/dependencies/bpf-tools/llvm/bin/llvm-ar"
export OBJDUMP="$bpf_sdk/dependencies/bpf-tools/llvm/bin/llvm-objdump"
export OBJCOPY="$bpf_sdk/dependencies/bpf-tools/llvm/bin/llvm-objcopy"

# Use the SDK's version of Rust to build for BPF
export RUSTUP_TOOLCHAIN=bpf
export RUSTFLAGS="
    -C lto=no \
    -C opt-level=2 \
    -C link-arg=-z -C link-arg=notext \
    -C link-arg=-T$bpf_sdk/rust/bpf.ld \
    -C link-arg=--Bdynamic \
    -C link-arg=-shared \
    -C link-arg=--threads=1 \
    -C link-arg=--entry=entrypoint \
    -C linker=$bpf_sdk/dependencies/bpf-tools/llvm/bin/ld.lld"
