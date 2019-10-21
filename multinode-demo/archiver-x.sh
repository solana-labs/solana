#!/usr/bin/env bash
#
# Start a dynamically-configured archiver
#

here=$(dirname "$0")
exec "$here"/archiver.sh --label x$$ "$@"
