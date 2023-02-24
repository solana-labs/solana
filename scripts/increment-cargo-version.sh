#!/usr/bin/env bash
set -e

usage() {
  cat <<EOF
usage: $0 [major|minor|patch|-preXYZ]

Increments the Cargo.toml version.

Default:
* Removes the prerelease tag if present, otherwise the minor version is incremented.
EOF
  exit 0
}

here="$(dirname "$0")"
cd "$here"/..
source ci/semver_bash/semver.sh
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
Cargo_tomls=($(find . -name Cargo.toml "${not_paths[@]}"))

# Collect the name of all the internal crates
crates=()
for Cargo_toml in "${Cargo_tomls[@]}"; do
  crates+=("$(readCargoVariable name "$Cargo_toml")")
done

# Read the current version
MAJOR=0
MINOR=0
PATCH=0
SPECIAL=""

semverParseInto "$(readCargoVariable version Cargo.toml)" MAJOR MINOR PATCH SPECIAL
[[ -n $MAJOR ]] || usage

currentVersion="$MAJOR\.$MINOR\.$PATCH$SPECIAL"

bump=$1
if [[ -z $bump ]]; then
  if [[ -n $SPECIAL ]]; then
    bump=dropspecial # Remove prerelease tag
  else
    bump=minor
  fi
fi
SPECIAL=""

# Figure out what to increment
case $bump in
patch)
  PATCH=$((PATCH + 1))
  ;;
major)
  MAJOR=$((MAJOR+ 1))
  MINOR=0
  PATCH=0
  ;;
minor)
  MINOR=$((MINOR+ 1))
  PATCH=0
  ;;
dropspecial)
  ;;
check)
  badTomls=()
  for Cargo_toml in "${Cargo_tomls[@]}"; do
    if grep "^version = { workspace = true }" "$Cargo_toml" &>/dev/null; then
      continue
    fi
    if ! grep "^version *= *\"$currentVersion\"$" "$Cargo_toml" &>/dev/null; then
      badTomls+=("$Cargo_toml")
    fi
  done
  if [[ ${#badTomls[@]} -ne 0 ]]; then
    echo "Error: Incorrect crate version specified in: ${badTomls[*]}"
    exit 1
  fi
  exit 0
  ;;
-*)
  if [[ $1 =~ ^-[A-Za-z0-9]*$ ]]; then
    SPECIAL="$1"
  else
    echo "Error: Unsupported characters found in $1"
    exit 1
  fi
  ;;
*)
  echo "Error: unknown argument: $1"
  usage
  ;;
esac

# Version bumps should occur in their own commit. Disallow bumping version
# in dirty working trees. Gate after arg parsing to prevent breaking the
# `check` subcommand.
(
  set +e
  if ! git diff --exit-code; then
    echo -e "\nError: Working tree is dirty. Commit or discard changes before bumping version." 1>&2
    exit 1
  fi
)

newVersion="$MAJOR.$MINOR.$PATCH$SPECIAL"

# Update all the Cargo.toml files
for Cargo_toml in "${Cargo_tomls[@]}"; do
  # ignore when version inheritant from workspace (exclude programs/sbf/Cargo.toml)
  if grep "^version = { workspace = true }" "$Cargo_toml" &>/dev/null && Cargo_toml && ! [[ $Cargo_toml =~ programs/sbf/Cargo.toml ]]; then
    continue
  fi

  # Set new crate version
  (
    set -x
    sed -i "$Cargo_toml" -e "0,/^version =/{s/^version = \"[^\"]*\"$/version = \"$newVersion\"/}"
  )

  # Fix up the version references to other internal crates
  for crate in "${crates[@]}"; do
    (
      set -x
      sed -i "$Cargo_toml" -e "
        s/^$crate = { *path *= *\"\([^\"]*\)\" *, *version *= *\"[^\"]*\"\(.*\)} *\$/$crate = \{ path = \"\1\", version = \"=$newVersion\"\2\}/
      "
    )
  done
done

# Update cargo lock files
scripts/cargo-for-all-lock-files.sh tree >/dev/null

echo "$currentVersion -> $newVersion"

exit 0
