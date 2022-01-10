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
