# |source| me

upload-ci-artifact() {
  echo "--- artifact: $1"
  if [[ -r "$1" ]]; then
    ls -l "$1"
    if ${BUILDKITE:-false}; then
      (
        set -x
        buildkite-agent artifact upload "$1"
      )
    fi
  else
    echo ^^^ +++
    echo "$1 not found"
  fi
}

