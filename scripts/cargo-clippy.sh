#!/usr/bin/env bash

# Runs `cargo clippy` in all individual workspaces in the repository.
#
# We have a number of clippy parameters that we want to enforce across the
# code base.  They are defined here.
#
# This script is run by the CI, so if you want to replicate what the CI is
# doing, better run this script, rather than calling `cargo clippy` manually.
#
# TODO It would be nice to provide arguments to narrow clippy checks to a single
# workspace and/or package.  To speed up the interactive workflow.

set -o errexit

here="$(dirname "$0")"

<<<<<<< HEAD
if [[ -z $cargo ]]; then
  echo >&2 "Failed to find cargo. Mac readlink doesn't support -f. Consider switching
  to gnu readlink with 'brew install coreutils' and then symlink greadlink as
  /usr/local/bin/readlink."
  exit 1
fi
=======
# stable
"$here/cargo-clippy-stable.sh"
>>>>>>> 5247be17a9 (ci: split cargo-clippy into stable and nightly (#34566))

# nightly
"$here/cargo-clippy-nightly.sh"
