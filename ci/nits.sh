#!/usr/bin/env bash
#
# Project nits enforced here
#
set -e

cd "$(dirname "$0")/.."
source ci/_

# please don't print from --lib...
declare prints=(
  'print!'
  'println!'
  'eprint!'
  'eprintln!'
)

if _ git grep "${prints[@]/#/-e }" src
then
    exit 1
fi
