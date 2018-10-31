
## Prerequisites

### Linux Ubuntu

The following link describes how to install llvm:
  https://blog.kowalczyk.info/article/k/how-to-install-latest-clang-6.0-on-ubuntu-16.04-xenial-wsl.html

### macOS

The following depends on Homebrew, instructions on how to install Homebrew are at https://brew.sh

Once Homebrew is installed, ensure the latest llvm is installed:
```
$ brew update        # <- ensure your brew is up to date
$ brew install llvm  # <- should output “Warning: llvm 7.0.0 is already installed and up-to-date”
$ brew --prefix llvm # <- should output “/usr/local/opt/llvm”
```

