
## Prerequisites

### Linux Ubuntu

The following link describes how to install llvm:
  https://blog.kowalczyk.info/article/k/how-to-install-latest-clang-6.0-on-ubuntu-16.04-xenial-wsl.html

### macOS

Ensure the latest llvm is installed:
```
$ brew update        # <- ensure your brew is up to date
$ brew list llvm     # <- should see "llvm: stable 7.0.0 (bottled), HEAD [keg-only]"
$ brew install llvm
$ brew --prefix llvm # <- should output "/usr/local/opt/llvm"
```

