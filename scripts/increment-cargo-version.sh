#!/usr/bin/env bash
set -e

usage() {
  cat <<EOF
usage: $0 [major|minor|patch|-preXYZ]

Increments the Cargo.toml version

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
  web3.js/examples
)

not_paths=()
for ignore in "${ignores[@]}"; do
  not_paths+=(-not -path "*/$ignore/*")
done

echo "Finding toml files"
# shellcheck disable=2207,SC2068 # Don't want a positional arg if `not-paths` is empty
Cargo_tomls=($(find . -mindepth 2 -name Cargo.toml ${not_paths[@]}))

echo "Finding markdown files"
# shellcheck disable=2207
markdownFiles=($(find . -name "*.md"))

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

semverParseInto "$(readCargoVariable version "${Cargo_tomls[0]}")" MAJOR MINOR PATCH SPECIAL
[[ -n $MAJOR ]] || usage

currentVersion="$MAJOR\.$MINOR\.$PATCH$SPECIAL"
currentVersionDisplay="$MAJOR.$MINOR.$PATCH$SPECIAL"

bump=$1
if [[ -z $bump ]]; then
  echo "Figuring current channel"
  eval "$(ci/channel-info.sh)"
  if [[ -z $CHANNEL ]]; then
    echo "Error: Unable to figure current channel"
    usage
  fi
  echo "CHANNEL=$CHANNEL"
  if [[ $CHANNEL = stable ]]; then
    bump="patch"
  elif [[ $CHANNEL = beta ]]; then
    bump=pre
  elif [[ $CHANNEL = edge ]]; then
    bump=minor
  else
    echo "Error: Unknown CHANNEL"
    exit 1
  fi
fi

# Figure out what to increment
case $bump in
patch)
  echo "Bumping patch version"

  if [[ -z $SPECIAL ]]; then
    PATCH=$((PATCH + 1))
  fi

  # Assume patch version bumps only occur on a stable channel.  Clear the
  # pre-release tag, if any
  #
  SPECIAL=""
  ;;
major)
  echo "Bumping major version"
  MAJOR=$((MAJOR + 1))
  MINOR=0
  PATCH=0

  # Assume major version bumps only occur on the master branch.  Reset back to
  # the first pre-release
  SPECIAL=".pre.0"
  ;;
minor)
  echo "Bumping minor version"
  MINOR=$((MINOR + 1))
  PATCH=0

  # Assume minor version bumps only occur on the master branch.  Reset back to
  # the first pre-release
  SPECIAL=".pre.0"
  ;;
pre)
  echo "Bumping pre-release version"

  # Pre version bumps happen on the edge and beta channels
  if [[ $SPECIAL =~ -pre.([0-9]*) ]]; then
    PRERELEASE=${BASH_REMATCH[1]}
    PRERELEASE=$((PRERELEASE + 1))
    SPECIAL="-pre-$PRERELEASE"
  else
    echo "Unsupported pre-release identifier: $SPECIAL"
    exit 1
  fi
  ;;
dropspecial)
  SPECIAL=""
  ;;
check)
  badTomls=()
  for Cargo_toml in "${Cargo_tomls[@]}"; do
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
  if [[ $1 =~ ^-[.A-Za-z0-9]*$ ]]; then
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

newVersion="$MAJOR.$MINOR.$PATCH$SPECIAL"

if [[ "$currentVersionDisplay" = "$newVersion" ]]; then
   echo "Nothing to do.  Current version is $currentVersionDisplay"
fi
echo "Bumping version to $newVersion"

# Update all the Cargo.toml files
for Cargo_toml in "${Cargo_tomls[@]}"; do
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
        s/^$crate = { *path *= *\"\([^\"]*\)\" *, *version *= *\"[^\"]*\"\(.*\)} *\$/$crate = \{ path = \"\1\", version = \"$newVersion\"\2\}/
      "
    )
  done
done

# Update all the documentation references
for file in "${markdownFiles[@]}"; do
  # Set new crate version
  (
    set -x
    sed -i "$file" -e "s/$currentVersion/$newVersion/g"
  )
done

echo "$currentVersionDisplay -> $newVersion"

exit 0
