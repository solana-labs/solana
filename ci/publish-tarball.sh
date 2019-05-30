#!/usr/bin/env bash
set -e

cd "$(dirname "$0")/.."

DRYRUN=
if [[ -z $BUILDKITE_BRANCH ]]; then
  DRYRUN="echo"
  CHANNEL=unknown
fi

eval "$(ci/channel-info.sh)"

TAG=
if [[ -n "$BUILDKITE_TAG" ]]; then
  CHANNEL_OR_TAG=$BUILDKITE_TAG
  TAG="$BUILDKITE_TAG"
elif [[ -n "$TRIGGERED_BUILDKITE_TAG" ]]; then
  CHANNEL_OR_TAG=$TRIGGERED_BUILDKITE_TAG
  TAG="$TRIGGERED_BUILDKITE_TAG"
else
  CHANNEL_OR_TAG=$CHANNEL
fi

if [[ -z $CHANNEL_OR_TAG ]]; then
  echo Unable to determine channel to publish into, exiting.
  exit 1
fi

case "$(uname)" in
Darwin)
  TARGET=x86_64-apple-darwin
  ;;
Linux)
  TARGET=x86_64-unknown-linux-gnu
  ;;
*)
  TARGET=unknown-unknown-unknown
  ;;
esac

echo --- Creating tarball
(
  set -x
  rm -rf solana-release/
  mkdir solana-release/

  COMMIT="$(git rev-parse HEAD)"

  (
    echo "channel: $CHANNEL_OR_TAG"
    echo "commit: $COMMIT"
    echo "target: $TARGET"
  ) > solana-release/version.yml

  source ci/rust-version.sh stable
  scripts/cargo-install-all.sh +"$rust_stable" solana-release

  rm -rf target/perf-libs
  ./fetch-perf-libs.sh
  mkdir solana-release/target
  cp -a target/perf-libs solana-release/target/

  # shellcheck source=/dev/null
  source ./target/perf-libs/env.sh
  (
    cd validator
    cargo +"$rust_stable" install --path . --features=cuda --root ../solana-release-cuda
  )
  cp solana-release-cuda/bin/solana-validator solana-release/bin/solana-validator-cuda
  cp -a scripts multinode-demo solana-release/

  # Add a wrapper script for validator.sh
  # TODO: Remove multinode/... from tarball
  cat > solana-release/bin/validator.sh <<'EOF'
#!/usr/bin/env bash
set -e
cd "$(dirname "$0")"/..
export USE_INSTALL=1
exec multinode-demo/validator.sh "$@"
EOF
  chmod +x solana-release/bin/validator.sh

  # Add a wrapper script for clear-config.sh
  # TODO: Remove multinode/... from tarball
  cat > solana-release/bin/clear-config.sh <<'EOF'
#!/usr/bin/env bash
set -e
cd "$(dirname "$0")"/..
export USE_INSTALL=1
exec multinode-demo/clear-config.sh "$@"
EOF
  chmod +x solana-release/bin/clear-config.sh

  tar jvcf solana-release-$TARGET.tar.bz2 solana-release/
  cp solana-release/bin/solana-install solana-install-$TARGET
)

echo --- Saving build artifacts
source ci/upload-ci-artifact.sh
upload-ci-artifact solana-release-$TARGET.tar.bz2

if [[ -n $DO_NOT_PUBLISH_TAR ]]; then
  echo Skipped due to DO_NOT_PUBLISH_TAR
  exit 0
fi

for file in solana-release-$TARGET.tar.bz2 solana-install-$TARGET; do
  echo --- AWS S3 Store: $file
  (
    set -x
    $DRYRUN docker run \
      --rm \
      --env AWS_ACCESS_KEY_ID \
      --env AWS_SECRET_ACCESS_KEY \
      --volume "$PWD:/solana" \
      eremite/aws-cli:2018.12.18 \
      /usr/bin/s3cmd --acl-public put /solana/"$file" s3://release.solana.com/"$CHANNEL_OR_TAG"/"$file"

    echo Published to:
    $DRYRUN ci/format-url.sh http://release.solana.com/"$CHANNEL_OR_TAG"/"$file"
  )

  if [[ -n $TAG ]]; then
    ci/upload-github-release-asset.sh $file
  fi
done

echo --- ok
