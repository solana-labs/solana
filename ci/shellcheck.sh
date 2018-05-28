#!/bin/bash -e
#
# Reference: https://github.com/koalaman/shellcheck/wiki/Directive

cd "$(dirname "$0")/.."

set -x
docker pull koalaman/shellcheck
find . -name "*.sh" -not -regex ".*/.cargo/.*" -print0 \
  | xargs -0 \
      docker run -w /work -v "$PWD:/work" \
        koalaman/shellcheck --color=always --external-sources --shell=bash

exit 0
