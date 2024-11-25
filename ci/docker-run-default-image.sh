#!/usr/bin/env bash

set -e

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# shellcheck disable=SC1091
source "$here/docker/env.sh"

"$here/docker-run.sh" "${CI_DOCKER_IMAGE:?}" "$@"
