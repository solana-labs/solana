#!/usr/bin/env bash
set -e
cd "$(dirname "$0")/.."
source ci/semver_bash/semver.sh

# Only package/publish if this is a tagged release
[[ -n $TRIGGERED_BUILDKITE_TAG ]] || {
  echo TRIGGERED_BUILDKITE_TAG unset, skipped
  exit 0
}

semverParseInto "$TRIGGERED_BUILDKITE_TAG" MAJOR MINOR PATCH SPECIAL
expectedCrateVersion="$MAJOR.$MINOR.$PATCH$SPECIAL"

[[ -n "$CRATES_IO_TOKEN" ]] || {
  echo CRATES_IO_TOKEN undefined
  exit 1
}

cargoCommand="cargo publish --token $CRATES_IO_TOKEN"

Cargo_tomls=$(ci/order-crates-for-publishing.py)

for Cargo_toml in "${Cargo_tomls[@]}"; do
  echo "-- $Cargo_toml"
  grep -q "^version = \"$expectedCrateVersion\"$" "$Cargo_toml" || {
    echo "Error: $Cargo_toml version is not $expectedCrateVersion"
    exit 1
  }

  (
    set -x
    crate=$(dirname "$Cargo_toml")
    # TODO: the rocksdb package does not build with the stock rust docker image,
    # so use the solana rust docker image until this is resolved upstream
    source ci/rust-version.sh
    ci/docker-run.sh "$rust_stable_docker_image" bash -exc "cd $crate; $cargoCommand"
    #ci/docker-run.sh rust bash -exc "cd $crate; $cargoCommand"
  ) || true # <-- Don't fail.  We want to be able to retry the job in cases when a publish fails halfway due to network/cloud issues
done

exit 0
