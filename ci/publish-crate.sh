#!/usr/bin/env bash
set -e
cd "$(dirname "$0")/.."


# List of internal crates to publish
#
# IMPORTANT: the order of the CRATES *is* significant.  Crates must be published
# before the crates that depend on them.  Note that this information is already
# expressed in the various Cargo.toml files, and ideally would not be duplicated
# here. (TODO: figure the crate ordering dynamically)
#
CRATES=(
  sdk
  keygen
  metrics
  drone
  programs/native/{budget,bpf_loader,lua_loader,native_loader,noop,system,vote}
  .
  fullnode
  genesis
  ledger-tool
  wallet
)


maybePackage="echo Package skipped"
maybePublish="echo Publish skipped"

# Only package/publish if this is a tagged release
if [[ -n $BUILDKITE_TAG && -n $TRIGGERED_BUILDKITE_TAG ]]; then
  maybePackage="cargo package"

  # Only publish if there's no human around
  if [[ -n $CI ]]; then
    maybePublish="cargo publish --token $CRATES_IO_TOKEN"
    if [[ -z "$CRATES_IO_TOKEN" ]]; then
      echo CRATES_IO_TOKEN undefined
      exit 1
    fi
  fi
fi

for crate in "${CRATES[@]}"; do
  if [[ ! -r $crate/Cargo.toml ]]; then
    echo "Error: $crate/Cargo.toml does not exist"
    exit 1
  fi
  echo "-- $crate"
  # TODO: Ensure the published version matches the contents of BUILDKITE_TAG
  (
    set -x
    ci/docker-run.sh rust bash -exc "cd $crate; $maybePackage; $maybePublish"
  )
done

exit 0
