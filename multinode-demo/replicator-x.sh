#!/usr/bin/env bash
#
# Start a dynamically-configured replicator
#

here=$(dirname "$0")
exec "$here"/replicator.sh --label x$$ "$@"
