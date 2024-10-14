#!/usr/bin/env bash

set -e

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

os_name="$1"

# shellcheck source=.github/scripts/install-openssl.sh
source "$here/install-openssl.sh" "$os_name"
# shellcheck source=.github/scripts/install-proto.sh
source "$here/install-proto.sh" "$os_name"
