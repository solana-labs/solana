#!/usr/bin/env bash
set -e
cd "$(dirname "$0")/.."
source ci/semver_bash/semver.sh
source ci/rust-version.sh stable

# shellcheck disable=SC2086
is_crate_version_uploaded() {
  name=$1
  version=$2
  curl https://crates.io/api/v1/crates/${name}/${version} | \
  python3 -c "import sys,json; print('version' in json.load(sys.stdin));"
}

# Only package/publish if this is a tagged release
[[ -n $CI_TAG ]] || {
  echo CI_TAG unset, skipped
  exit 0
}

semverParseInto "$CI_TAG" MAJOR MINOR PATCH SPECIAL
expectedCrateVersion="$MAJOR.$MINOR.$PATCH$SPECIAL"

[[ -n "$CRATES_IO_TOKEN" ]] || {
  echo CRATES_IO_TOKEN undefined
  exit 1
}

Cargo_tomls=$(ci/order-crates-for-publishing.py)

for Cargo_toml in $Cargo_tomls; do
  echo "--- $Cargo_toml"
  grep -q "^version = \"$expectedCrateVersion\"$" "$Cargo_toml" || {
    echo "Error: $Cargo_toml version is not $expectedCrateVersion"
    exit 1
  }

  crate_name=$(grep -m 1 '^name = ' "$Cargo_toml" | cut -f 3 -d ' ' | tr -d \")

  if grep -q "^publish = false" "$Cargo_toml"; then
    echo "$crate_name is is marked as unpublishable"
    continue
  fi

  if [[ $(is_crate_version_uploaded "$crate_name" "$expectedCrateVersion") = True ]] ; then
    echo "${crate_name} version ${expectedCrateVersion} is already on crates.io"
    continue
  fi

  (
    set -x
    crate=$(dirname "$Cargo_toml")
    # The rocksdb package does not build with the stock rust docker image so use
    # the solana rust docker image
    cargoCommand="cargo publish --token $CRATES_IO_TOKEN"
    ci/docker-run.sh "$rust_stable_docker_image" bash -exc "cd $crate; $cargoCommand"
  ) || true # <-- Don't fail.  We want to be able to retry the job in cases when a publish fails halfway due to network/cloud issues

  numRetries=30
  for ((i = 1 ; i <= numRetries ; i++)); do
    echo "Attempt ${i} of ${numRetries}"
    if [[ $(is_crate_version_uploaded "$crate_name" "$expectedCrateVersion") = True ]] ; then
      echo "Found ${crate_name} version ${expectedCrateVersion} on crates.io REST API"

      really_uploaded=0
      (
        set -x
        rm -rf crate-test
        cargo +"$rust_stable" init crate-test
        cd crate-test/
        echo "${crate_name} = \"${expectedCrateVersion}\"" >> Cargo.toml
        echo "[workspace]" >> Cargo.toml
        cargo +"$rust_stable" check
      ) && really_uploaded=1
      if ((really_uploaded)); then
        break;
      fi
      echo "${crate_name} not yet available for download from crates.io"
    fi
    echo "Did not find ${crate_name} version ${expectedCrateVersion} on crates.io.  Sleeping for 2 seconds."
    sleep 2
  done
done

exit 0
