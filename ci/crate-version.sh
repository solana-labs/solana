#!/usr/bin/env bash
#
# Outputs the current crate version from a given Cargo.toml
#
set -e

Cargo_toml=$1
[[ -n $Cargo_toml ]] || {
  echo "Usage: $0 path/to/Cargo.toml"
  exit 0
}

[[ -r $Cargo_toml ]] || {
  echo "Error: unable to read $Cargo_toml"
  exit 1
}

while read -r name equals value _; do
  if [[ $name = version && $equals = = ]]; then
    echo "${value//\"/}"
    exit 0
  fi
done < <(cat "$Cargo_toml")

echo Unable to locate version in Cargo.toml 1>&2
exit 1
