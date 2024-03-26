#!/usr/bin/env bash

set -e

os_name="$1"

case "$os_name" in
"Windows")
  vcpkg install openssl:x64-windows-static-md
  vcpkg integrate install
  choco install protoc
  export PROTOC='C:\ProgramData\chocolatey\lib\protoc\tools\bin\protoc.exe'
  ;;
"macOS")
  brew install protobuf
  ;;
"Linux") ;;
*)
  echo "Unknown Operating System"
  ;;
esac
