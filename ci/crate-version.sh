#!/bin/bash -e
#
# Outputs the current crate version
#

cd "$(dirname "$0")"/..

while read -r name equals value _; do
  if [[ $name = version && $equals = = ]]; then
    echo "${value//\"/}"
    exit 0
  fi
done < <(cat Cargo.toml)

echo Unable to locate version in Cargo.toml 1>&2
exit 1
