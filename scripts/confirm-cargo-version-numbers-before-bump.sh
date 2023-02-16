#!/usr/bin/env bash
set -e

usage() {
  cat <<EOF
usage: $0 branch tag

Checks that the tag matches the branch (unless branch is master) and the Cargo.toml versions match the tag.
EOF
  exit 0
}

branch="$1"
tag="$2"

[[ -n $tag ]] || usage
echo "branch: $branch tag: $tag"

# The tag is expected to be the branch name plus a patch number (unless branch is master). eg:
#   tag:    v1.2.3
#   branch: v1.2
 if [[ "$tag" != "$branch"* && $branch != "master" ]]; then
    >&2 echo "Tag must start with the branch name (unless branch is master). Tag: $tag   Branch: $branch"
    exit 1
fi

here="$(dirname "$0")"
cd "$here"/..
source scripts/read-cargo-variable.sh

ignores=(
  .cache
  .cargo
  target
  node_modules
)

not_paths=()
for ignore in "${ignores[@]}"; do
  not_paths+=(-not -path "*/$ignore/*")
done

# shellcheck disable=2207
Cargo_tomls=($(find . -mindepth 2 -name Cargo.toml "${not_paths[@]}"))

for Cargo_toml in "${Cargo_tomls[@]}"; do
  manifest_version="$(readCargoVariable version "${Cargo_toml}")"
  if ! [[ "v$manifest_version" == "$tag" ]]; then
    >&2 echo "Tag must match the crate version in the manifest files. Mismatch found in $Cargo_toml. Tag: $tag   Manifest version: $manifest_version"
    exit 1
  else
    echo "tag matches manifest: $Cargo_toml $manifest_version $tag"
  fi
done
