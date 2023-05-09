#
# Configures the SBF SDK environment
#

if [ -z "$sbf_sdk" ]; then
  sbf_sdk=.
fi

# Ensure the sdk is installed
"$sbf_sdk"/scripts/install.sh

# Use the SDK's version of llvm to build the compiler-builtins for SBF
export CC="$sbf_sdk/dependencies/platform-tools/llvm/bin/clang"
export AR="$sbf_sdk/dependencies/platform-tools/llvm/bin/llvm-ar"
export OBJDUMP="$sbf_sdk/dependencies/platform-tools/llvm/bin/llvm-objdump"
export OBJCOPY="$sbf_sdk/dependencies/platform-tools/llvm/bin/llvm-objcopy"
