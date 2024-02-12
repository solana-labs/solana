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
    args=(
      --rm
      --env AWS_ACCESS_KEY_ID
      --env AWS_SECRET_ACCESS_KEY
      --volume "$PWD:/solana"

    )
    if [[ $(uname -m) = arm64 ]]; then
      # Ref: https://blog.jaimyn.dev/how-to-build-multi-architecture-docker-images-on-an-m1-mac/#tldr
      args+=(
        --platform linux/amd64
      )
    fi
    args+=(
      amazon/aws-cli:2.13.11
      s3 cp "$1" "$2" --acl public-read
    )
    set -x
    docker run "${args[@]}"
  )
}

upload-gcs-artifact() {
  echo "--- artifact: $1 to $2"
  docker run --rm \
    -v "$GCS_RELEASE_BUCKET_WRITER_CREDIENTIAL:/application_default_credentials.json" \
    -v "$PWD:/solana" \
    -e CLOUDSDK_AUTH_CREDENTIAL_FILE_OVERRIDE=/application_default_credentials.json \
    gcr.io/google.com/cloudsdktool/google-cloud-cli:latest \
    gcloud storage cp "$1" "$2"
}
