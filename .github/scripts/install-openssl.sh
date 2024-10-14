#!/usr/bin/env bash

set -e

os_name="$1"

case "$os_name" in
"Windows")
  choco install openssl --version 3.3.2 --install-arguments="'/DIR=C:\OpenSSL'" -y
  export OPENSSL_LIB_DIR="C:\OpenSSL\lib\VC\x64\MT"
  export OPENSSL_INCLUDE_DIR="C:\OpenSSL\include"
  ;;
"macOS") ;;
"Linux") ;;
*)
  echo "Unknown Operating System"
  ;;
esac
