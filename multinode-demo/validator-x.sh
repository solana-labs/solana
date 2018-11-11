#!/usr/bin/env bash
#
# Start a dynamically-configured validator node
#

here=$(dirname "$0")

exec "$here"/validator.sh -x "$@"
