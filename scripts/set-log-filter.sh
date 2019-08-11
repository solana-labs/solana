#!/usr/bin/env bash
#
# Reconfigures the log filter on a validator using the current RUST_LOG value
#

if [[ -n $1 ]]; then
  url=$1
else
  # Default to the local node
  url=http://127.0.0.1:8899
fi

if [[ -z $RUST_LOG ]]; then
  echo "RUST_LOG not defined"
  exit 1
fi

set -x
exec curl $url -X POST -H "Content-Type: application/json" \
  -d "
    {
      \"jsonrpc\": \"2.0\",
      \"id\": 1,
      \"method\": \"setLogFilter\",
      \"params\": [\"$RUST_LOG\"]
    }
  "
