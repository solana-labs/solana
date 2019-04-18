#!/usr/bin/env bash
#
# Start a dynamically-configured full node
#

here=$(dirname "$0")

exec "$here"/fullnode.sh --label x$$ "$@"
