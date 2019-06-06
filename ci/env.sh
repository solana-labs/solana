#
# Normalized CI environment variables
#
# |source| me
#

if ${CI:-false}; then
  if ${TRAVIS:-false}; then
    export CI_BRANCH=$TRAVIS_BRANCH
    export CI_BUILD_ID=$TRAVIS_BUILD_ID
    export CI_COMMIT=$TRAVIS_COMMIT
    export CI_JOB_ID=$TRAVIS_JOB_ID
    if $TRAVIS_PULL_REQUEST; then
      export CI_PULL_REQUEST=true
    else
      export CI_PULL_REQUEST=
    fi
    export CI_OS_NAME=$TRAVIS_OS_NAME
    export CI_TAG=$TRAVIS_TAG
  fi
  if ${BUILDKITE:-false}; then
    export CI_BRANCH=$BUILDKITE_BRANCH
    export CI_BUILD_ID=$BUILDKITE_BUILD_ID
    export CI_COMMIT=$BUILDKITE_COMMIT
    export CI_JOB_ID=$BUILDKITE_JOB_ID
    # The standard BUILDKITE_PULL_REQUEST environment variable is always "false" due
    # to how solana-ci-gate is used to trigger PR builds rather than using the
    # standard Buildkite PR trigger.
    if [[ $CI_BRANCH =~ pull/* ]]; then
      export CI_PULL_REQUEST=true
    else
      export CI_PULL_REQUEST=
    fi
    export CI_OS_NAME=linux
    # TRIGGERED_BUILDKITE_TAG is a workaround to propagate BUILDKITE_TAG into
    # the solana-secondary builder
    if [[ -n $TRIGGERED_BUILDKITE_TAG ]]; then
      export CI_TAG=$TRIGGERED_BUILDKITE_TAG
    else
      export CI_TAG=$BUILDKITE_TAG
    fi
  fi
else
  export CI=
  export CI_BRANCH=
  export CI_BUILD_ID=
  export CI_COMMIT=
  export CI_JOB_ID=
  export CI_OS_NAME=
  export CI_PULL_REQUEST=
  export CI_TAG=
fi

cat <<EOF
CI=$CI
CI_BRANCH=$CI_BRANCH
CI_BUILD_ID=$CI_BUILD_ID
CI_COMMIT=$CI_COMMIT
CI_JOB_ID=$CI_JOB_ID
CI_OS_NAME=$CI_OS_NAME
CI_PULL_REQUEST=$CI_PULL_REQUEST
CI_TAG=$CI_TAG
EOF
