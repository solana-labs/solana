
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

