#!/usr/bin/env bash
set -e

cd "$(dirname "$0")/.."

if [[ -n $APPVEYOR ]]; then
  # Bootstrap rust build environment
  source ci/env.sh
  source ci/rust-version.sh

  appveyor DownloadFile https://win.rustup.rs/ -FileName rustup-init.exe
  export USERPROFILE="D:\\"
  ./rustup-init -yv --default-toolchain $rust_stable --default-host x86_64-pc-windows-msvc
  export PATH="$PATH:/d/.cargo/bin"
  rustc -vV
  cargo -vV
fi

DRYRUN=
if [[ -z $CI_BRANCH ]]; then
  DRYRUN="echo"
  CHANNEL=unknown
fi

eval "$(ci/channel-info.sh)"

TAG=
if [[ -n "$CI_TAG" ]]; then
  CHANNEL_OR_TAG=$CI_TAG
  TAG="$CI_TAG"
else
  CHANNEL_OR_TAG=$CHANNEL
fi

if [[ -z $CHANNEL_OR_TAG ]]; then
  echo +++ Unable to determine channel or tag to publish into, exiting.
  exit 0
fi

case "$CI_OS_NAME" in
osx)
  TARGET=x86_64-apple-darwin
  ;;
linux)
  TARGET=x86_64-unknown-linux-gnu
  ;;
windows)
  TARGET=x86_64-pc-windows-gnu
  ;;
*)
  echo CI_OS_NAME unset
  exit 1
  ;;
esac

RELEASE_BASENAME="${RELEASE_BASENAME:=solana-release}"
TARBALL_BASENAME="${TARBALL_BASENAME:="$RELEASE_BASENAME"}"

echo --- Creating release tarball
(
  set -x
  rm -rf "${RELEASE_BASENAME:?}"/
  mkdir "${RELEASE_BASENAME}"/

  COMMIT="$(git rev-parse HEAD)"

  (
    echo "channel: $CHANNEL_OR_TAG"
    echo "commit: $COMMIT"
    echo "target: $TARGET"
  ) > "${RELEASE_BASENAME}"/version.yml

  # Make CHANNEL available to include in the software version information
  export CHANNEL

  source ci/rust-version.sh stable
  scripts/cargo-install-all.sh +"$rust_stable" "${RELEASE_BASENAME}"

  tar cvf "${TARBALL_BASENAME}"-$TARGET.tar "${RELEASE_BASENAME}"
  bzip2 "${TARBALL_BASENAME}"-$TARGET.tar
  cp "${RELEASE_BASENAME}"/bin/solana-install-init solana-install-init-$TARGET
  cp "${RELEASE_BASENAME}"/version.yml "${TARBALL_BASENAME}"-$TARGET.yml
)

# Metrics tarball is platform agnostic, only publish it from Linux
MAYBE_TARBALLS=
if [[ "$CI_OS_NAME" = linux ]]; then
  metrics/create-metrics-tarball.sh
  (
    set -x
    sdk/bpf/scripts/package.sh
    [[ -f bpf-sdk.tar.bz2 ]]

  )
  MAYBE_TARBALLS="bpf-sdk.tar.bz2 solana-metrics.tar.bz2"
fi

source ci/upload-ci-artifact.sh

for file in "${TARBALL_BASENAME}"-$TARGET.tar.bz2 "${TARBALL_BASENAME}"-$TARGET.yml solana-install-init-"$TARGET"* $MAYBE_TARBALLS; do
  if [[ -n $DO_NOT_PUBLISH_TAR ]]; then
    upload-ci-artifact "$file"
    echo "Skipped $file due to DO_NOT_PUBLISH_TAR"
    continue
  fi

  if [[ -n $BUILDKITE ]]; then
    echo --- AWS S3 Store: "$file"
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
      ci/upload-github-release-asset.sh "$file"
    fi
  elif [[ -n $TRAVIS ]]; then
    # .travis.yml uploads everything in the travis-s3-upload/ directory to release.solana.com
    mkdir -p travis-s3-upload/"$CHANNEL_OR_TAG"
    cp -v "$file" travis-s3-upload/"$CHANNEL_OR_TAG"/

    if [[ -n $TAG ]]; then
      # .travis.yaml uploads everything in the travis-release-upload/ directory to
      # the associated Github Release
      mkdir -p travis-release-upload/
      cp -v "$file" travis-release-upload/
    fi
  elif [[ -n $APPVEYOR ]]; then
    # Add artifacts for .appveyor.yml to upload
    appveyor PushArtifact "$file" -FileName "$CHANNEL_OR_TAG"/"$file"
  fi
done

echo --- ok
