#!/bin/bash -e

cd "$(dirname "$0")"/..

if [[ -z $BUILDKITE ]]; then
  echo BUILDKITE not defined
  exit 1
fi

if [[ -z $SOLANA_METRICS_CONFIG ]]; then
  echo SOLANA_METRICS_CONFIG not defined
  exit 1
fi

if [[ -z $TESTNET ]]; then
  TESTNET=$(buildkite-agent meta-data get "testnet" --default "")
fi

if [[ -z $TESTNET_OP ]]; then
  TESTNET_OP=$(buildkite-agent meta-data get "testnet-operation" --default "")
fi

if [[ -z $TESTNET || -z $TESTNET_OP ]]; then
  (
    cat <<EOF
steps:
  - block: "Manage Testnet"
    fields:
      - select: "Network"
        key: "testnet"
        options:
          - label: "testnet"
            value: "testnet"
          - label: "testnet-perf"
            value: "testnet-perf"
          - label: "testnet-master"
            value: "testnet-master"
          - label: "testnet-master-perf"
            value: "testnet-master-perf"
      - select: "Operation"
        key: "testnet-operation"
        default: "sanity-or-restart"
        options:
          - label: "Sanity check.  Restart network on failure"
            value: "sanity-or-restart"
          - label: "Start (or restart) the network"
            value: "start"
          - label: "Stop the network"
            value: "stop"
          - label: "Sanity check only"
            value: "sanity"
  - command: "ci/$(basename "$0")"
    agents:
      - "queue=$BUILDKITE_AGENT_META_DATA_QUEUE"
EOF
  ) | buildkite-agent pipeline upload
  exit 0
fi

ci/channel-info.sh
eval "$(ci/channel-info.sh)"
set # <-- remove

case $TESTNET in
testnet-master|testnet-master-perf)
  CHANNEL=edge
  CHANNEL_BRANCH=$EDGE_CHANNEL
  ;;
testnet|testnet-perf)
  CHANNEL=beta
  CHANNEL_BRANCH=$BETA_CHANNEL
  ;;
*)
  echo "Error: Invalid TESTNET=$TESTNET"
  exit 1
  ;;
esac

if [[ $BUILDKITE_BRANCH != "$CHANNEL_BRANCH" ]]; then
  (
    cat <<EOF
steps:
  - trigger: "$BUILDKITE_PIPELINE_SLUG"
    async: true
    build:
      message: "$BUILDKITE_MESSAGE"
      branch: "$CHANNEL_BRANCH"
      env:
        TESTNET: "$TESTNET"
        TESTNET_OP: "$TESTNET_OP"
EOF
  ) | buildkite-agent pipeline upload
  exit 0
fi


sanity() {
  echo "--- sanity $TESTNET"
  case $TESTNET in
  testnet-master-perf)
    (
      set -ex
      ci/testnet-sanity.sh master-perf-testnet-solana-com us-west1-b
    )
    ;;
  testnet-master)
    # shellcheck disable=2030
    (
      set -ex
      export REJECT_EXTRA_NODES=1
      export NO_LEDGER_VERIFY=1
      export NO_VALIDATOR_SANITY=1
      ci/testnet-sanity.sh master-testnet-solana-com us-west1-b
    )
    ;;
  testnet)
    # shellcheck disable=2030
    # shellcheck disable=2031
    (
      set -ex
      export NO_LEDGER_VERIFY=1
      export NO_VALIDATOR_SANITY=1
      ci/testnet-sanity.sh testnet-solana-com us-east1-c
    )
    ;;
  testnet-perf)
    # shellcheck disable=2030
    # shellcheck disable=2031
    (
      set -ex
      export REJECT_EXTRA_NODES=1
      export NO_LEDGER_VERIFY=1
      export NO_VALIDATOR_SANITY=1
      ci/testnet-sanity.sh perf-testnet-solana-com us-west1-b
    )
    echo tp
    ;;
  *)
    echo "Error: Invalid TESTNET=$TESTNET"
    exit 1
    ;;
  esac
}


start() {
  declare maybeDelete=$1
  if [[ -z $maybeDelete ]]; then
    echo "--- start $TESTNET"
  else
    echo "--- stop $TESTNET"
  fi

  case $TESTNET in
  testnet-master-perf)
    (
      set -ex
      ci/testnet-deploy.sh master-perf-testnet-solana-com us-west1-b \
        -G "n1-standard-16 --accelerator count=2,type=nvidia-tesla-v100" \
        -t $CHANNEL -c 2 \
        ${maybeDelete:+-d}
    )
    ;;
  testnet-master)
    # shellcheck disable=2030
    # shellcheck disable=2031
    (
      set -ex
      export NO_LEDGER_VERIFY=1
      export NO_VALIDATOR_SANITY=1
      ci/testnet-deploy.sh master-testnet-solana-com us-west1-b \
        -s $CHANNEL -n 3 -c 0 -P -a master-testnet-solana-com \
        ${maybeDelete:+-d}
    )
    ;;
  testnet)
    # shellcheck disable=2030
    # shellcheck disable=2031
    (
      set -ex
      export NO_LEDGER_VERIFY=1
      export NO_VALIDATOR_SANITY=1
      ci/testnet-deploy.sh testnet-solana-com us-east1-c \
        -s $CHANNEL -n 3 -g -c 0 -P -a testnet-solana-com  \
        ${maybeDelete:+-d}
    )
    ;;
  testnet-perf)
    # shellcheck disable=2030
    # shellcheck disable=2031
    (
      set -ex
      export NO_LEDGER_VERIFY=1
      export NO_VALIDATOR_SANITY=1
      ci/testnet-deploy.sh perf-testnet-solana-com us-west1-b \
        -G "n1-standard-16 --accelerator count=2,type=nvidia-tesla-v100" \
        -t $CHANNEL -c 2 \
        ${maybeDelete:+-d}
    )
    ;;
  *)
    echo "Error: Invalid TESTNET=$TESTNET"
    exit 1
    ;;
  esac
}

stop() {
  start delete
}

case $TESTNET_OP in
sanity)
  sanity
  ;;
start)
  start
  ;;
stop)
  stop
  ;;
sanity-or-restart)
  if sanity; then
    echo Pass
  else
    echo "Sanity failed, restarting the network"
    echo "^^^ +++"
    start
  fi
  ;;
esac

echo --- fin
exit 0
