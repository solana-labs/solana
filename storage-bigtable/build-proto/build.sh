#!/usr/bin/env bash

set -ex
cd "$(dirname "$0")"

cargo="$(readlink -f "../../cargo")"

if [[ ! -d googleapis ]]; then
  git clone https://github.com/googleapis/googleapis.git
fi

exec "$cargo" run
