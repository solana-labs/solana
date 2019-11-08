#!/usr/bin/env bash
#
# Reference: https://github.com/koalaman/shellcheck/wiki/Directive
set -e

cd "$(dirname "$0")/.."
(
  set -x
  git ls-files -- '*.sh' ':(exclude)ci/semver_bash' \
    | xargs ci/docker-run.sh koalaman/shellcheck --color=always --external-sources --shell=bash
)
echo --- ok
