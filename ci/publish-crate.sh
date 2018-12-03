#!/usr/bin/env bash
set -e

cd "$(dirname "$0")/.."

if [[ -z "$BUILDKITE_TAG" && -z "$TRIGGERED_BUILDKITE_TAG" ]]; then
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
for Cargo_toml in {sdk,metrics,drone,programs/native/{bpf_loader,lua_loader,noop,system_program},.}/Cargo.toml; do
  # TODO: Ensure the published version matches the contents of BUILDKITE_TAG
  (
    set -x
    ci/docker-run.sh rust bash -exc "cd $(dirname "$Cargo_toml"); cargo package; $maybePublish"
  )
done

exit 0
