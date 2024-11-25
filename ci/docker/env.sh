#!/usr/bin/env bash

ci_docker_env_sh_here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# shellcheck disable=SC1091
source "${ci_docker_env_sh_here}/../rust-version.sh"

if [[ -z "${rust_stable}" || -z "${rust_nightly}" ]]; then
  echo "Error: rust_stable or rust_nightly is empty. Please check rust-version.sh." >&2
  exit 1
fi

export BASE_IMAGE=ubuntu:22.04
export RUST_VERSION="${rust_stable}"
export RUST_NIGHTLY_VERSION="${rust_nightly}"
export GOLANG_VERSION=1.21.3
export NODE_MAJOR=18
export SCCACHE_VERSION=v0.8.1
export GRCOV_VERSION=v0.8.18

hash_vars=(
  "${BASE_IMAGE}"
  "${RUST_VERSION}"
  "${RUST_NIGHTLY_VERSION}"
  "${GOLANG_VERSION}"
  "${NODE_MAJOR}"
  "${SCCACHE_VERSION}"
  "${GRCOV_VERSION}"
)
hash_input=$(IFS="_"; echo "${hash_vars[*]}")
ci_docker_hash=$(echo -n "${hash_input}" | sha256sum | head -c 8)

SANITIZED_BASE_IMAGE="${BASE_IMAGE//:/-}"
export CI_DOCKER_IMAGE="anzaxyz/ci:${SANITIZED_BASE_IMAGE}_rust-${RUST_VERSION}_${RUST_NIGHTLY_VERSION}_${ci_docker_hash}"
