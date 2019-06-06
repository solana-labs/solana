#!/usr/bin/env bash
#
# Computes the current branch names of the edge, beta and stable
# channels, as well as the latest tagged release for beta and stable.
#
# stdout of this script may be eval-ed
#

here="$(dirname "$0")"

# shellcheck source=ci/semver_bash/semver.sh
source "$here"/semver_bash/semver.sh

remote=https://github.com/solana-labs/solana.git

# Fetch all vX.Y.Z tags
#
# NOTE: pre-release tags are explicitly ignored
#
# shellcheck disable=SC2207
tags=( \
  $(git ls-remote --tags $remote \
    | cut -c52- \
    | grep '^v[[:digit:]][[:digit:]]*\.[[:digit:]][[:digit:]]*.[[:digit:]][[:digit:]]*$' \
    | cut -c2- \
  ) \
)

# Fetch all the vX.Y branches
#
# shellcheck disable=SC2207
heads=( \
  $(git ls-remote --heads $remote \
    | cut -c53- \
    | grep '^v[[:digit:]][[:digit:]]*\.[[:digit:]][[:digit:]]*$' \
    | cut -c2- \
  ) \
)

# Figure the beta channel by looking for the largest vX.Y branch
beta=
for head in "${heads[@]}"; do
  if [[ -n $beta ]]; then
    if semverLT "$head.0" "$beta.0"; then
      continue
    fi
  fi
  beta=$head
done

# Figure the stable channel by looking for the second largest vX.Y branch
stable=
for head in "${heads[@]}"; do
  if [[ $head = "$beta" ]]; then
    continue
  fi
  if [[ -n $stable ]]; then
    if semverLT "$head.0" "$stable.0"; then
      continue
    fi
  fi
  stable=$head
done

for tag in "${tags[@]}"; do
  if [[ -n $beta && $tag = $beta* ]]; then
    if [[ -n $beta_tag ]]; then
      if semverLT "$tag" "$beta_tag"; then
        continue
      fi
    fi
    beta_tag=$tag
  fi

  if [[ -n $stable && $tag = $stable* ]]; then
    if [[ -n $stable_tag ]]; then
      if semverLT "$tag" "$stable_tag"; then
        continue
      fi
    fi
    stable_tag=$tag
  fi
done

EDGE_CHANNEL=master
BETA_CHANNEL=${beta:+v$beta}
STABLE_CHANNEL=${stable:+v$stable}
BETA_CHANNEL_LATEST_TAG=${beta_tag:+v$beta_tag}
STABLE_CHANNEL_LATEST_TAG=${stable_tag:+v$stable_tag}


if [[ $CI_BRANCH = "$STABLE_CHANNEL" ]]; then
  CHANNEL=stable
elif [[ $CI_BRANCH = "$EDGE_CHANNEL" ]]; then
  CHANNEL=edge
elif [[ $CI_BRANCH = "$BETA_CHANNEL" ]]; then
  CHANNEL=beta
fi

echo EDGE_CHANNEL="$EDGE_CHANNEL"
echo BETA_CHANNEL="$BETA_CHANNEL"
echo BETA_CHANNEL_LATEST_TAG="$BETA_CHANNEL_LATEST_TAG"
echo STABLE_CHANNEL="$STABLE_CHANNEL"
echo STABLE_CHANNEL_LATEST_TAG="$STABLE_CHANNEL_LATEST_TAG"
echo CHANNEL="$CHANNEL"

exit 0
