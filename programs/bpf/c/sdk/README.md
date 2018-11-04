
## Prerequisites

## LLVM / clang 7.0.0
http://releases.llvm.org/download.html

### Linux Ubuntu 16.04 (xenial)
```
$ wget -O - https://apt.llvm.org/llvm-snapshot.gpg.key | sudo apt-key add -
$ sudo apt-add-repository "deb http://apt.llvm.org/xenial/ llvm-toolchain-xenial-7 main"
$ sudo apt-get update
$ sudo apt-get install -y clang-7
```

### Linux Ubuntu 14.04 (trusty)
```
$ wget -O - https://apt.llvm.org/llvm-snapshot.gpg.key | sudo apt-key add -
$ sudo apt-add-repository "deb http://apt.llvm.org/trusty/ llvm-toolchain-trusty-7 main"
$ sudo apt-get update
$ sudo apt-get install -y clang-7
```

### macOS
The following depends on Homebrew, instructions on how to install Homebrew are at https://brew.sh

Once Homebrew is installed, ensure the latest llvm is installed:
```
$ brew update        # <- ensure your brew is up to date
$ brew install llvm  # <- should output “Warning: llvm 7.0.0 is already installed and up-to-date”
$ brew --prefix llvm # <- should output “/usr/local/opt/llvm”
```

## Development

### Quick start
To get started create a `makefile` containing:
```make
include path/to/bpf.mk
```
and `src/program.c` containing:
```c
#include <solana_sdk.h>

bool entrypoint(const uint8_t *input) {
  SolKeyedAccounts ka[1];
  uint8_t *data;
  uint64_t data_len;

  if (!sol_deserialize(buf, ka, SOL_ARRAY_SIZE(ka), NULL, &data, &data_len)) {
    return false;
  }
  print_params(1, ka, data, data_len);
  return true;
}
```

Then run `make` to build `out/program.o`.
Run `make help` for more details.

### Limitations
* Programs must be fully contained within a single .c file
* No libc is available but `solana_sdk.h` provides a minimal set of
primitives.
