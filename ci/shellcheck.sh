#!/usr/bin/env bash
#
# Reference: https://github.com/koalaman/shellcheck/wiki/Directive
set -e

cd "$(dirname "$0")/.."
(
  set -x
  git ls-files -- '*.sh' ':(exclude)ci/semver_bash' \
    | xargs ci/docker-run.sh koalaman/shellcheck@sha256:fe24ab9a9b6b62d3adb162f4a80e006b6a63cae8c6ffafbae45772bab85e7294 --color=always --external-sources --shell=bash
)
echo --- ok
