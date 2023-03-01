#!/usr/bin/env bash
set -e

here="$(dirname "$0")"

#shellcheck source=ci/downstream-projects/func-example-helloworld.sh
source "$here"/func-example-helloworld.sh

#shellcheck source=ci/downstream-projects/func-spl.sh
source "$here"/func-spl.sh

#shellcheck source=ci/downstream-projects/func-openbook-dex.sh
source "$here"/func-openbook-dex.sh

#shellcheck source=ci/downstream-projects/common.sh
source "$here"/common.sh

_ example_helloworld
_ spl
_ openbook_dex
