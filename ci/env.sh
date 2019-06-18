#
# Normalized CI environment variables
#
# |source| me
#

if [[ -n $CI ]]; then
  export CI=1
  if [[ -n $TRAVIS ]]; then
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
    export CI_REPO_SLUG=$TRAVIS_REPO_SLUG
    export CI_TAG=$TRAVIS_TAG
  elif [[ -n $BUILDKITE ]]; then
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
    export CI_REPO_SLUG=$BUILDKITE_ORGANIZATION_SLUG/$BUILDKITE_PIPELINE_SLUG
    # TRIGGERED_BUILDKITE_TAG is a workaround to propagate BUILDKITE_TAG into
    # the solana-secondary builder
    if [[ -n $TRIGGERED_BUILDKITE_TAG ]]; then
      export CI_TAG=$TRIGGERED_BUILDKITE_TAG
    else
      export CI_TAG=$BUILDKITE_TAG
    fi
  elif [[ -n $APPVEYOR ]]; then
    export CI_BRANCH=$APPVEYOR_REPO_BRANCH
    export CI_BUILD_ID=$APPVEYOR_BUILD_ID
    export CI_COMMIT=$APPVEYOR_REPO_COMMIT
    export CI_JOB_ID=$APPVEYOR_JOB_ID
    if [[ -n $APPVEYOR_PULL_REQUEST_NUMBER ]]; then
      export CI_PULL_REQUEST=true
    else
      export CI_PULL_REQUEST=
    fi
    if [[ $CI_LINUX = True ]]; then
      export CI_OS_NAME=linux
    elif [[ $CI_WINDOWS = True ]]; then
      export CI_OS_NAME=windows
    fi
    export CI_REPO_SLUG=$APPVEYOR_REPO_NAME
    export CI_TAG=$APPVEYOR_REPO_TAG_NAME
  fi
else
  export CI=
  export CI_BRANCH=
  export CI_BUILD_ID=
  export CI_COMMIT=
  export CI_JOB_ID=
  export CI_OS_NAME=
  export CI_PULL_REQUEST=
  export CI_REPO_SLUG=
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
