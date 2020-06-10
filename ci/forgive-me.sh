#!/usr/bin/env bash

set -x

here="$(realpath "$(dirname "$0")")"
source_dir="${here:?}/.."

committish="${1:?}"
tarballBaseName="${2:-solana-release}"
remote=https://github.com/solana-labs/solana.git

cleanup_tmp_source_dir() {
  rm -rf "$tmp_source_dir"
}

set -e
tmp_source_dir="$(mktemp --directory --tmpdir="$source_dir")"

trap 'cleanup_tmp_source_dir' EXIT

# Clone the specified committish
git clone "$remote" "$tmp_source_dir"

if pushd "$tmp_source_dir"; then
  (
    set +e

    git fetch origin "$committish":branch_to_build
    git checkout branch_to_build

    # Use our scripts
    cp -ar "${source_dir}"/{fetch-perf-libs.sh,scripts,net,multinode-demo,ci} "$tmp_source_dir"

    export CHANNEL=edge
    export DO_NOT_PUBLISH_TAR=true # Never publish
    export TARBALL_BASENAME="$tarballBaseName"
    ci/publish-tarball.sh

    cp "${tarballBaseName}"*.tar.bz2 "${source_dir}"
  )
  popd
fi
