#!/usr/bin/env bash

os_name="$1"

case "$os_name" in
"Windows")
  # openssl
  choco install openssl
  if [[ -d "C:\Program Files\OpenSSL" ]]; then
    echo "OPENSSL_DIR: C:\Program Files\OpenSSL"
    echo "OPENSSL_DIR=C:\Program Files\OpenSSL" >>$GITHUB_ENV
  elif [[ -d "C:\Program Files\OpenSSL-Win64" ]]; then
    echo "OPENSSL_DIR: C:\Program Files\OpenSSL-Win64"
    echo "OPENSSL_DIR=C:\Program Files\OpenSSL-Win64" >>$GITHUB_ENV
  else
    echo "can't determine OPENSSL_DIR"
    exit 1
  fi

  # protoc
  choco install protoc
  echo "PROTOC=C:\ProgramData\chocolatey\lib\protoc\tools\bin\protoc.exe" >>$GITHUB_ENV
  ;;
"macOS")
  brew install protobuf
  ;;
"Linux") ;;
*)
  echo "Unknown Operating System"
  ;;
esac
