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

upload-s3-artifact() {
  echo "--- artifact: $1 to $2"
  (
    set -x
    docker run \
      --rm \
      --env AWS_ACCESS_KEY_ID \
      --env AWS_SECRET_ACCESS_KEY \
      --volume "$PWD:/solana" \
      eremite/aws-cli:2018.12.18 \
      /usr/bin/s3cmd --acl-public put "$1" "$2"
  )
}
