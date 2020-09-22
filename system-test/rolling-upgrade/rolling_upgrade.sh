#!/usr/bin/env bash

DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" >/dev/null 2>&1 && pwd )"
REPO_ROOT=${DIR}/../..

# shellcheck source=/dev/null # Ignore generated source target
source "${REPO_ROOT}/net/config/config"
# shellcheck source=system-test/automation_utils.sh
source "${REPO_ROOT}/system-test/automation_utils.sh"

NET_SH="${REPO_ROOT}/net/net.sh"

set -x

: "${UPGRADE_INITIAL_DELAY:=0}" # Time to wait before starting rolling upgrade
: "${UPGRADE_INTERVALIDATOR_DELAY:?}" # Time to wait between validators during upgrade
: "${UPGRADE_POST_TEST_DELAY:=0}" #Time to wait after upgrade

sleep_if_positive() {
  declare delay=$1
  if [[ "$delay" -gt 0 ]]; then
    sleep "$delay"
  fi
}

check_sanity() {
  (
    set -e
    "$NET_SH" sanity
  )
}

# Fetch new software and upload it to the bootstrap validator
(
  declare -g sw_version_args
  get_net_launch_software_version_launch_args "$UPGRADE_CHANNEL" "upgrade-release" sw_version_args
  if [[ -n $LOCAL_BUILD_BRANCH || -n $LOCAL_BUILD_REVISION ]]; then
    unset LOCAL_BUILD_BRANCH
    unset LOCAL_BUILD_REVISION
  fi
  if [[ -n $UPGRADE_LOCAL_BUILD_BRANCH ]]; then
    export LOCAL_BUILD_BRANCH="$UPGRADE_LOCAL_BUILD_BRANCH"
  fi
  if [[ -n $UPGRADE_LOCAL_BUILD_REVISION ]]; then
    export LOCAL_BUILD_REVISION="$UPGRADE_LOCAL_BUILD_REVISION"
  fi

  # shellcheck disable=2086 # $sw_version_args holds two args. Don't quote!
  "$NET_SH" upgrade $sw_version_args
)

# Wait initial delay, if any
sleep_if_positive "$UPGRADE_INITIAL_DELAY"

if [[ "$UPGRADE_INITIAL_DELAY" -gt 0 ]]; then
  check_sanity
fi

# Restart validators one by one
# shellcheck disable=SC2154 # sourced from config above
for i in "${!validatorIpList[@]}"; do
  if [[ "$i" -eq 0 ]]; then
    # net.sh doesn't support restarting the bootstrap validator yet
    continue
  fi

  declare ipAddress="${validatorIpList[$i]}"

  "$NET_SH" stopnode -i "$ipAddress"
  "$NET_SH" startnode -r -i "$ipAddress"

  # This could be replaced with something based on `solana catchup`
  sleep_if_positive "$UPGRADE_INTERVALIDATOR_DELAY"

  check_sanity
done

sleep_if_positive "$UPGRADE_POST_TEST_DELAY"

if [[ "$UPGRADE_POST_TEST_DELAY" -gt 0 ]]; then
  check_sanity
fi
