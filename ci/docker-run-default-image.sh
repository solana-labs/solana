#!/usr/bin/env bash

set -e

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# shellcheck disable=SC1091
source "$here/rust-version.sh"

"$here/docker-run.sh" "${ci_docker_image:?}" "$@"
