#!/usr/bin/env bash
set -e

here="$(dirname "$0")"

#shellcheck source=ci/downstream-projects/func-example-helloworld.sh
source "$here"/func-example-helloworld.sh

#shellcheck source=ci/downstream-projects/common.sh
source "$here"/common.sh

_ example_helloworld
