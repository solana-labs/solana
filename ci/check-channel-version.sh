 # Ensure the current channel version is not equal ("greater") than
# the version of the latest tag
if [[ -z $CI_TAG ]]; then
  echo "--- channel version check"
  (
    eval "$(ci/channel-info.sh)"

    if [[ -n $CHANNEL_LATEST_TAG ]]; then
      source scripts/read-cargo-variable.sh

      version=$(readCargoVariable version Cargo.toml)
      echo "latest channel tag: $CHANNEL_LATEST_TAG"
      echo "current version: v$version"

      if [[ $CHANNEL_LATEST_TAG = v$version ]]; then
        echo -e "\033[31mError:\033[0m A release has been tagged since your feature branch was last rebased. <current version> must be greater than <latest channel tag>..
        Possible solutions (in the order they should be tried):
        1. rebase your feature branch on the base branch
        2. merge the PR: \"Bump Version to ...\" once it has passed ci/checks, then rebase
        3. ask for help in #devops on discord"
        exit 1
      fi
    else
      echo "Skipped. CHANNEL_LATEST_TAG (CHANNEL=$CHANNEL) unset"
    fi
  )
fi
