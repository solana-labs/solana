#!/usr/bin/env bash
#
# Adjust the maximum number of files that may be opened to as large as possible.
#

maxOpenFds=65000
if [[ $(uname) = Darwin ]]; then
  maxOpenFds=24576 # Appears to be the max permitted on macOS...
fi

if [[ $(ulimit -n) -lt $maxOpenFds ]]; then
  ulimit -n $maxOpenFds || {
    echo "Error: nofiles too small: $(ulimit -n). Run \"ulimit -n $maxOpenFds\" to continue";
    exit 1
  }
fi

