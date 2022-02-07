# How to build Solana on windows

## 1. Enable symlink on git config

Because `build.rs` use symlinks to refer to other build files, you will need to enable symlinks on git.

```bash
git config core.symlinks true
git reset --hard
```

You might need to run CMD terminal in `Administrator` mode to avoid `permission denied errors` when creating symlinks.

## 2. Install OpenSSL package on windows
* Download and install non-light version of [OenSSL](http://slproweb.com/products/Win32OpenSSL.html), i.e. installed in `C:\OpenSSL-Win64`.
* Download and install root [certificate](https://curl.se/docs/caextract.html), i.e. installed in `C:\OpenSSL-Win64\certs\cacert.pem`.
* Add the following environment variables:
```bash
set OPENSSL_NO_VENDOR=1
set OPENSSL_DIR=C:\OpenSSL-Win64
set SSL_CERT_FILE=C:\OpenSSL-Win64\certs\cacert.pem
```

## 3. Install LLVM package on windows
* Download and install [LLVM](https://releases.llvm.org/download.html), i.e. installed in `C:\LLVM`.
* Add the following environment variables:
```bash
set LIBCLANG_PATH=C:\LLVM\bin
```

Alternatively, you can create a `setenv.bat` to set all the environment variables:
```bash
@echo off
REM  set up build environment

:: set up openssl env variable
set OPENSSL_NO_VENDOR=1
set OPENSSL_DIR=C:\OpenSSL-Win64
set SSL_CERT_FILE=C:\OpenSSL-Win64\certs\cacert.pem

:: setup libclang path
set LIBCLANG_PATH=C:\LLVM\bin
```

Now, you should be able to build like:
```
setenv.bat && cargo build
```