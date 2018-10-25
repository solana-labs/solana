#!/bin/bash -e

usage() {
  cat <<EOF
usage: $0 [major|minor|patch|-preXYZ]

Increments the Cargo.toml version.
A minor version increment is the default
EOF
  exit 0
}

here="$(dirname "$0")"
cd "$here"/..
source ci/semver_bash/semver.sh

readCargoVersion() {
  declare Cargo_toml="$1"

  while read -r version equals semver _; do
    if [[ $version = version && $equals = = ]]; then
      echo "${semver//\"/}"
      return
    fi
  done < <(cat "$Cargo_toml")
  echo "Unable to locate version in $Cargo_toml" 1>&2
}

MAJOR=0
MINOR=0
PATCH=0
SPECIAL=""
semverParseInto "$(readCargoVersion ./Cargo.toml)" MAJOR MINOR PATCH SPECIAL
[[ -n $MAJOR ]] || usage

currentVersion="$MAJOR.$MINOR.$PATCH$SPECIAL"
SPECIAL=""

case ${1:-minor} in
patch)
  PATCH=$((PATCH + 1))
  ;;
major)
  MAJOR=$((MAJOR+ 1))
  ;;
minor)
  MINOR=$((MINOR+ 1))
  ;;
-*)
  SPECIAL="$1"
  ;;
*)
  echo "Error: unknown argument: $1"
  usage
  ;;
esac

newVersion="$MAJOR.$MINOR.$PATCH$SPECIAL"

# shellcheck disable=2044 # Disable 'For loops over find output are fragile...'
for Cargo_toml in $(find . -name Cargo.toml); do
  # Bump crate version
  (
    set -x
    sed -i "$Cargo_toml" -e "s/^version = \"[^\"]*\"$/version = \"$newVersion\"/"
  )

  # Fix up the internal references to the solana_program_interface crate
  (
    set -x
    sed -i "$Cargo_toml" -e "
      s/^solana_program_interface.*\(\"[^\"]*common\"\).*\$/solana_program_interface = \{ path = \1, version = \"$newVersion\" \}/
    "
  )
done

echo "$currentVersion -> $newVersion"

exit 0
