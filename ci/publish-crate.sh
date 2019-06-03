#!/usr/bin/env bash
set -e
cd "$(dirname "$0")/.."
source ci/semver_bash/semver.sh

# shellcheck disable=SC2086
is_crate_version_uploaded() {
  name=$1
  version=$2
  curl https://crates.io/api/v1/crates/${name}/${version} | \
  python3 -c "import sys,json; print('version' in json.load(sys.stdin));"
}

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

for Cargo_toml in $Cargo_tomls; do
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
  ) || true # <-- Don't fail.  We want to be able to retry the job in cases when a publish fails halfway due to network/cloud issues

  # shellcheck disable=SC2086
  crate_name=$(grep -m 1 '^name = ' $Cargo_toml | cut -f 3 -d ' ' | tr -d \")
  numRetries=30
  for ((i = 1 ; i <= numRetries ; i++)); do
    echo "Attempt ${i} of ${numRetries}"
    # shellcheck disable=SC2086
    if [[ $(is_crate_version_uploaded $crate_name $expectedCrateVersion) = True ]] ; then
      echo "Found ${crate_name} version ${expectedCrateVersion} on crates.io"
      break
    fi
    echo "Did not find ${crate_name} version ${expectedCrateVersion} on crates.io.  Sleeping for 2 seconds."
    sleep 2
  done
done

exit 0
