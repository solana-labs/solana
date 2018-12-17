#!/usr/bin/env bash
set -e

if [[ ! -x ./grcov ]]; then
  uname=$(uname | tr '[:upper:]' '[:lower:]')
  if [[ $uname = darwin ]]; then
    uname="osx"
  fi
  uname_m=$(uname -m | tr '[:upper:]' '[:lower:]')
  name=grcov-${uname}-${uname_m}.tar.bz2

  wget "https://github.com/mozilla/grcov/releases/download/v0.3.2/$name"
  tar xjf "$name"
fi

ls -lh grcov
