#!/usr/bin/env bash
#
# Reference: https://github.com/koalaman/shellcheck/wiki/Directive
set -e

cd "$(dirname "$0")/.."
(
  set -x
  find . -name "*.sh" \
      -not -regex ".*/ci/semver_bash/.*" \
      -not -regex ".*/.cargo/.*" \
      -not -regex ".*/node_modules/.*" \
      -not -regex ".*/target/.*" \
      -print0 \
    | xargs -0 \
        ci/docker-run.sh koalaman/shellcheck@sha256:fe24ab9a9b6b62d3adb162f4a80e006b6a63cae8c6ffafbae45772bab85e7294 --color=always --external-sources --shell=bash
)
echo --- ok
