#!/usr/bin/env bash
#
# |cargo install| of the top-level crate will not install binaries for
# other workspace creates.
set -e
cd "$(dirname "$0")/.."

set -x
cargo install --path drone "$@"
cargo install --path . "$@"
