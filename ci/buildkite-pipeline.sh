#!/usr/bin/env bash
#
# Builds a buildkite pipeline based on the environment variables
#

set -e
cd "$(dirname "$0")"/..

output_file=${1:-/dev/stderr}

if [[ -n $CI_PULL_REQUEST ]]; then
  IFS=':' read -ra affected_files <<< "$(buildkite-agent meta-data get affected_files)"
  if [[ ${#affected_files[*]} -eq 0 ]]; then
    echo "Unable to determine the files affected by this PR"
    exit 1
  fi
else
  affected_files=()
fi

annotate() {
  if [[ -n $BUILDKITE ]]; then
    buildkite-agent annotate "$@"
  fi
}

# Assume everyting needs to be tested when this file or any Dockerfile changes
mandatory_affected_files=()
mandatory_affected_files+=(^ci/buildkite-pipeline.sh)
mandatory_affected_files+=(^ci/docker-rust/Dockerfile)
mandatory_affected_files+=(^ci/docker-rust-nightly/Dockerfile)

# Checks if a CI pull request affects one or more path patterns.  Each
# pattern argument is checked in series. If one of them found to be affected,
# return immediately as such.
#
# Bash regular expressions are permitted in the pattern:
#     affects .rs$    -- any file or directory ending in .rs
#     affects .rs     -- also matches foo.rs.bar
#     affects ^snap/  -- anything under the snap/ subdirectory
#     affects snap/   -- also matches foo/snap/
# Any pattern starting with the ! character will be negated:
#     affects !^docs/  -- anything *not* under the docs/ subdirectory
#
affects() {
  if [[ -z $CI_PULL_REQUEST ]]; then
    # affected_files metadata is not currently available for non-PR builds so assume
    # the worse (affected)
    return 0
  fi
  for pattern in "${mandatory_affected_files[@]}" "$@"; do
    if [[ ${pattern:0:1} = "!" ]]; then
      for file in "${affected_files[@]}"; do
        if [[ ! $file =~ ${pattern:1} ]]; then
          return 0 # affected
        fi
      done
    else
      for file in "${affected_files[@]}"; do
        if [[ $file =~ $pattern ]]; then
          return 0 # affected
        fi
      done
    fi
  done

  return 1 # not affected
}


# Checks if a CI pull request affects anything other than the provided path patterns
#
# Syntax is the same as `affects()` except that the negation prefix is not
# supported
#
affects_other_than() {
  if [[ -z $CI_PULL_REQUEST ]]; then
    # affected_files metadata is not currently available for non-PR builds so assume
    # the worse (affected)
    return 0
  fi

  for file in "${affected_files[@]}"; do
    declare matched=false
    for pattern in "$@"; do
        if [[ $file =~ $pattern ]]; then
          matched=true
        fi
    done
    if ! $matched; then
      return 0 # affected
    fi
  done

  return 1 # not affected
}


start_pipeline() {
  echo "# $*" > "$output_file"
  echo "steps:" >> "$output_file"
}

command_step() {
  cat >> "$output_file" <<EOF
  - name: "$1"
    command: "$2"
    timeout_in_minutes: $3
    artifact_paths: "log-*.txt"
    agents:
      queue: "solana"
EOF
}


trigger_secondary_step() {
  cat  >> "$output_file" <<"EOF"
  - trigger: "solana-secondary"
    branches: "!pull/*"
    async: true
    build:
      message: "${BUILDKITE_MESSAGE}"
      commit: "${BUILDKITE_COMMIT}"
      branch: "${BUILDKITE_BRANCH}"
      env:
        TRIGGERED_BUILDKITE_TAG: "${BUILDKITE_TAG}"
EOF
}

wait_step() {
  echo "  - wait" >> "$output_file"
}

all_test_steps() {

  # BPF test suite
  if affects \
             .rs$ \
             Cargo.lock$ \
             Cargo.toml$ \
             ^ci/rust-version.sh \
             ^ci/test-stable-bpf.sh \
             ^ci/test-stable.sh \
             ^ci/test-local-cluster.sh \
             ^core/build.rs \
             ^fetch-perf-libs.sh \
             ^programs/ \
             ^sdk/ \
      ; then
    cat >> "$output_file" <<"EOF"
  - command: "ci/test-stable-bpf.sh"
    name: "stable-bpf"
    timeout_in_minutes: 35
    artifact_paths: "bpf-dumps.tar.bz2"
    agents:
      queue: "solana"
EOF
  else
    annotate --style info \
      "Stable-BPF skipped as no relevant files were modified"
  fi

}

pull_or_push_steps() {
  command_step sanity "ci/test-sanity.sh" 5
  wait_step

  # Check for any .sh file changes
  if affects .sh$; then
    command_step shellcheck "ci/shellcheck.sh" 5
    wait_step
  fi

  # Version bump PRs are an edge case that can skip most of the CI steps
  if affects .toml$ && affects .lock$ && ! affects_other_than .toml$ .lock$; then
    optional_old_version_number=$(git diff "$BUILDKITE_PULL_REQUEST_BASE_BRANCH"..HEAD validator/Cargo.toml | \
      grep -e "^-version" | sed  's/-version = "\(.*\)"/\1/')
    echo "optional_old_version_number: ->$optional_old_version_number<-"
    new_version_number=$(grep -e  "^version = " validator/Cargo.toml | sed 's/version = "\(.*\)"/\1/')
    echo "new_version_number: ->$new_version_number<-"

    # Every line in a version bump diff will match one of these patterns. Since we're using grep -v the output is the
    # lines that don't match. Any diff that produces output here is not a version bump.
    # | cat is a no-op. If this pull request is a version bump then grep will output no lines and have an exit code of 1.
    # Piping the output to cat prevents that non-zero exit code from exiting this script
    diff_other_than_version_bump=$(git diff "$BUILDKITE_PULL_REQUEST_BASE_BRANCH"..HEAD | \
      grep -vE "^ |^@@ |^--- |^\+\+\+ |^index |^diff |^-( \")?solana.*$optional_old_version_number|^\+( \")?solana.*$new_version_number|^-version|^\+version"|cat)
    echo "diff_other_than_version_bump: ->$diff_other_than_version_bump<-"

    if [ -z "$diff_other_than_version_bump" ]; then
      echo "Diff only contains version bump."
      command_step checks ". ci/rust-version.sh; ci/docker-run.sh \$\$rust_nightly_docker_image ci/test-checks.sh" 20
      exit 0
    fi
  fi

  # Run the full test suite by default, skipping only if modifications are local
  # to some particular areas of the tree
  if affects_other_than ^.buildkite ^.mergify .md$ ^docs/ ^web3.js/ ^explorer/ ^.gitbook; then
    all_test_steps
  fi

  # web3.js, explorer and docs changes run on Travis or Github actions...
}


if [[ -n $BUILDKITE_TAG ]]; then
  start_pipeline "Tag pipeline for $BUILDKITE_TAG"

  annotate --style info --context release-tag \
    "https://github.com/solana-labs/solana/releases/$BUILDKITE_TAG"

  # Jump directly to the secondary build to publish release artifacts quickly
  trigger_secondary_step
  exit 0
fi


if [[ $BUILDKITE_BRANCH =~ ^pull ]]; then
  echo "+++ Affected files in this PR"
  for file in "${affected_files[@]}"; do
    echo "- $file"
  done

  start_pipeline "Pull request pipeline for $BUILDKITE_BRANCH"

  # Add helpful link back to the corresponding Github Pull Request
  annotate --style info --context pr-backlink \
    "Github Pull Request: https://github.com/solana-labs/solana/$BUILDKITE_BRANCH"

  if [[ $GITHUB_USER = "dependabot[bot]" ]]; then
    command_step dependabot "ci/dependabot-pr.sh" 5
    wait_step
  fi
  pull_or_push_steps
  exit 0
fi

start_pipeline "Push pipeline for ${BUILDKITE_BRANCH:-?unknown branch?}"
pull_or_push_steps
wait_step
trigger_secondary_step
exit 0
