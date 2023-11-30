#!/usr/bin/env bash

set -e

os_name="$1"

case "$os_name" in
"Windows")
  ;;
"macOS")
  brew install protobuf
  ;;
"Linux") ;;
*)
  echo "Unknown Operating System"
  ;;
esac
