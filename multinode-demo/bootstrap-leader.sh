#!/usr/bin/env bash
#
# Start the bootstrap leader node
#

here=$(dirname "$0")
exec "$here"/fullnode.sh --bootstrap-leader x$$ "$@"
