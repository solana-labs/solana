CI_BUILD_START=$(date +%s)
export CI_BUILD_START

source ci/env.sh

#
# Kill any running docker containers, which are potentially left over from the
# previous CI job
#
(
  containers=$(docker ps -q)
  if [[ $(hostname) != metrics-solana-com && -n $containers ]]; then
    echo "+++ Killing stale docker containers"
    docker ps

    # shellcheck disable=SC2086 # Don't want to double quote $containers
    docker kill $containers
  fi
)

# Processes from previously aborted CI jobs seem to loiter, unclear why as one
# would expect the buildkite-agent to clean up all child processes of the
# aborted CI job.
# But as a workaround for now manually kill some known loiterers.  These
# processes will all have the `init` process as their PPID:
(
  victims=
  for name in bash cargo docker solana; do
    victims="$victims $(pgrep -u "$(id -u)" -P 1 -d \  $name)"
  done
  for victim in $victims; do
    echo "Killing pid $victim"
    kill -9 "$victim" || true
  done
)

# HACK: These are in our docker images, need to be removed from CARGO_HOME
#  because we try to cache downloads across builds with CARGO_HOME
# cargo lacks a facility for "system" tooling, always tries CARGO_HOME first
cargo uninstall cargo-audit || true
cargo uninstall svgbob_cli || true
cargo uninstall mdbook || true
