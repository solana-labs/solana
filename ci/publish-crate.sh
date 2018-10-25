#!/bin/bash -e

cd "$(dirname "$0")/.."

if [[ -z "$BUILDKITE_TAG" ]]; then
  # Skip publish if this is not a tagged release
  exit 0
fi

if [[ -z "$CRATES_IO_TOKEN" ]]; then
  echo CRATES_IO_TOKEN undefined
  exit 1
fi

maybePublish="echo Publish skipped"
if [[ -n $CI ]]; then
  maybePublish="cargo publish --token $CRATES_IO_TOKEN"
fi

# shellcheck disable=2044 # Disable 'For loops over find output are fragile...'
for Cargo_toml in {common,programs/native/{bpf_loader,lua_loader,noop}}/Cargo.toml; do
  # TODO: Ensure the published version matches the contents of BUILDKITE_TAG
  (
    set -x
    ci/docker-run.sh rust bash -exc "cd $(dirname "$Cargo_toml"); cargo package; $maybePublish"
  )
done

exit 0
