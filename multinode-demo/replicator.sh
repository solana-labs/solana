#!/usr/bin/env bash
#
# Start a relpicator
#

here=$(dirname "$0")
exec "$here"/fullnode.sh --replicator "$@"


