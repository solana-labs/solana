#!/usr/bin/env bash
#
# Start a dynamically-configured validator
#

here=$(dirname "$0")
exec "$here"/validator.sh --label x$$ "$@"
