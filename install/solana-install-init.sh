#!/bin/sh
# Copyright 2016 The Rust Project Developers. See the COPYRIGHT
# file at the top-level directory of this distribution and at
# http://rust-lang.org/COPYRIGHT.
#
# Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
# http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
# <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
# option. This file may not be copied, modified, or distributed
# except according to those terms.

# This is just a little script that can be downloaded from the internet to
# install solana-install. It just does platform detection, downloads the installer
# and runs it.

{ # this ensures the entire script is downloaded #

if [ -z "$SOLANA_DOWNLOAD_ROOT" ]; then
    SOLANA_DOWNLOAD_ROOT="https://github.com/solana-labs/solana/releases/download/"
fi
GH_LATEST_RELEASE="https://api.github.com/repos/solana-labs/solana/releases/latest"

set -e

usage() {
    cat 1>&2 <<EOF
solana-install-init
initializes a new installation

USAGE:
    solana-install-init [FLAGS] [OPTIONS] --data_dir <PATH> --pubkey <PUBKEY>

FLAGS:
    -h, --help              Prints help information
        --no-modify-path    Don't configure the PATH environment variable

OPTIONS:
    -d, --data_dir <PATH>    Directory to store install data
    -u, --url <URL>          JSON RPC URL for the solana cluster
    -p, --pubkey <PUBKEY>    Public key of the update manifest
EOF
}

main() {
    downloader --check
    need_cmd uname
    need_cmd mktemp
    need_cmd chmod
    need_cmd mkdir
    need_cmd rm
    need_cmd sed
    need_cmd grep

    for arg in "$@"; do
      case "$arg" in
        -h|--help)
          usage
          exit 0
          ;;
        *)
          ;;
      esac
    done

    case "$(uname)" in
    Linux)
      TARGET=x86_64-unknown-linux-gnu
      ;;
    Darwin)
      TARGET=x86_64-apple-darwin
      ;;
    *)
      err "machine architecture is currently unsupported"
      ;;
    esac

    temp_dir="$(mktemp -d 2>/dev/null || ensure mktemp -d -t solana-install-init)"
    ensure mkdir -p "$temp_dir"

    # Check for SOLANA_RELEASE environment variable override.  Otherwise fetch
    # the latest release tag from github
    if [ -n "$SOLANA_RELEASE" ]; then
      release="$SOLANA_RELEASE"
    else
      release_file="$temp_dir/release"
      printf 'looking for latest release\n' 1>&2
      ensure downloader "$GH_LATEST_RELEASE" "$release_file"
      release=$(\
        grep -m 1 \"tag_name\": "$release_file" \
        | sed -ne 's/^ *"tag_name": "\([^"]*\)",$/\1/p' \
      )
      if [ -z "$release" ]; then
        err 'Unable to figure latest release'
      fi
    fi

    download_url="$SOLANA_DOWNLOAD_ROOT/$release/solana-install-init-$TARGET"
    solana_install_init="$temp_dir/solana-install-init"

    printf 'downloading %s installer\n' "$release" 1>&2

    ensure mkdir -p "$temp_dir"
    ensure downloader "$download_url" "$solana_install_init"
    ensure chmod u+x "$solana_install_init"
    if [ ! -x "$solana_install_init" ]; then
        printf '%s\n' "Cannot execute $solana_install_init (likely because of mounting /tmp as noexec)." 1>&2
        printf '%s\n' "Please copy the file to a location where you can execute binaries and run ./solana-install-init." 1>&2
        exit 1
    fi

    ignore "$solana_install_init" "$@"
    retval=$?

    ignore rm "$solana_install_init"
    ignore rm -rf "$temp_dir"

    return "$retval"
}

err() {
    printf 'solana-install-init: %s\n' "$1" >&2
    exit 1
}

need_cmd() {
    if ! check_cmd "$1"; then
        err "need '$1' (command not found)"
    fi
}

check_cmd() {
    command -v "$1" > /dev/null 2>&1
}

# Run a command that should never fail. If the command fails execution
# will immediately terminate with an error showing the failing
# command.
ensure() {
    if ! "$@"; then
      err "command failed: $*"
    fi
}

# This is just for indicating that commands' results are being
# intentionally ignored. Usually, because it's being executed
# as part of error handling.
ignore() {
    "$@"
}

# This wraps curl or wget. Try curl first, if not installed,
# use wget instead.
downloader() {
    if check_cmd curl; then
        program=curl
    elif check_cmd wget; then
        program=wget
    else
        program='curl or wget' # to be used in error message of need_cmd
    fi

    if [ "$1" = --check ]; then
        need_cmd "$program"
    elif [ "$program" = curl ]; then
        curl -sSfL "$1" -o "$2"
    elif [ "$program" = wget ]; then
        wget "$1" -O "$2"
    else
        err "Unknown downloader"   # should not reach here
    fi
}

main "$@"

} # this ensures the entire script is downloaded #
