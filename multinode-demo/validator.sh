#!/usr/bin/env bash

here=$(dirname "$0")
exec "$here"/fullnode.sh --validator "$@"
