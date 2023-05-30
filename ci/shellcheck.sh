#!/usr/bin/env bash
#
# Reference: https://github.com/koalaman/shellcheck/wiki/Directive
set -e

cd "$(dirname "$0")/.."
(
  set -x
  git ls-files -- '.buildkite/hooks' '*.sh' ':(exclude)ci/semver_bash' \
    | xargs ci/docker-run.sh koalaman/shellcheck:v0.8.0 --color=always --external-sources --shell=bash
)
echo --- ok
