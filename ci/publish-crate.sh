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

# check workspace.version for worksapce root
workspace_cargo_tomls=(Cargo.toml programs/sbf/Cargo.toml)
for cargo_toml in "${workspace_cargo_tomls[@]}"; do
  if ! grep -q "^version = \"$expectedCrateVersion\"$" "$cargo_toml"; then
    echo "Error: Cargo.toml version is not $expectedCrateVersion"
    exit 1
  fi
done

Cargo_tomls=$(ci/order-crates-for-publishing.py)

for Cargo_toml in $Cargo_tomls; do
  echo "--- $Cargo_toml"

  # check the version which doesn't inherit from worksapce
  if ! grep -q "^version = { workspace = true }$" "$Cargo_toml"; then
    echo "Warn: $Cargo_toml doesn't use the inherited version"
    grep -q "^version = \"$expectedCrateVersion\"$" "$Cargo_toml" || {
      echo "Error: $Cargo_toml version is not $expectedCrateVersion"
      exit 1
    }
  fi

  crate_name=$(grep -m 1 '^name = ' "$Cargo_toml" | cut -f 3 -d ' ' | tr -d \")

  if grep -q "^publish = false" "$Cargo_toml"; then
    echo "$crate_name is marked as unpublishable"
    continue
  fi

  if [[ $(is_crate_version_uploaded "$crate_name" "$expectedCrateVersion") = True ]] ; then
    echo "${crate_name} version ${expectedCrateVersion} is already on crates.io"
    continue
  fi

  (
    set -x

    crate=$(dirname "$Cargo_toml")
    cargoCommand="cargo publish --token $CRATES_IO_TOKEN"

    numRetries=10
    for ((i = 1; i <= numRetries; i++)); do
      echo "Attempt ${i} of ${numRetries}"
      # The rocksdb package does not build with the stock rust docker image so use
      # the solana rust docker image
      if ci/docker-run-default-image.sh bash -exc "cd $crate; $cargoCommand"; then
        break
      fi

      if [ "$i" -lt "$numRetries" ]; then
        sleep 3
      else
        echo "couldn't publish '$crate_name'"
        exit 1
      fi
    done
  )
done

exit 0
