#!/bin/bash
here=$(dirname "$0")

exec "$here"/validator.sh -x "$@"
