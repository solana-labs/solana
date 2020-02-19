#!/usr/bin/env bash
set -e

cd "$(dirname "$0")"/..

if [[ -z $BUILDKITE ]]; then
  echo BUILDKITE not defined
  exit 1
fi

if [[ -z $SOLANA_METRICS_PARTIAL_CONFIG ]]; then
  echo SOLANA_METRICS_PARTIAL_CONFIG not defined
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
          - label: "testnet-edge"
            value: "testnet-edge"
          - label: "testnet-beta"
            value: "testnet-beta"
      - select: "Operation"
        key: "testnet-operation"
        default: "sanity-or-restart"
        options:
          - label: "Create testnet and then start software.  If the testnet already exists it will be deleted and re-created"
            value: "create-and-start"
          - label: "Create testnet, but do not start software.  If the testnet already exists it will be deleted and re-created"
            value: "create"
          - label: "Start network software on an existing testnet.  If software is already running it will be restarted"
            value: "start"
          - label: "Stop network software without deleting testnet nodes"
            value: "stop"
          - label: "Update the network software.  Restart network software on failure"
            value: "update-or-restart"
          - label: "Sanity check.  Restart network software on failure"
            value: "sanity-or-restart"
          - label: "Sanity check only"
            value: "sanity"
          - label: "Delete the testnet"
            value: "delete"
          - label: "Enable/unlock the testnet"
            value: "enable"
          - label: "Delete and then lock the testnet from further operation until it is re-enabled"
            value: "disable"
  - command: "ci/$(basename "$0")"
    agents:
      - "queue=$BUILDKITE_AGENT_META_DATA_QUEUE"
EOF
  ) | buildkite-agent pipeline upload
  exit 0
fi

ci/channel-info.sh
eval "$(ci/channel-info.sh)"


EC2_ZONES=(
  us-west-1a
  us-west-2a
  us-east-1a
  us-east-2a
  sa-east-1a
  eu-west-1a
  eu-west-2a
  eu-central-1a
  ap-northeast-2a
  ap-southeast-2a
  ap-south-1a
  ca-central-1a
)

# GCE zones with _lots_ of quota
GCE_ZONES=(
  us-west1-a
  us-central1-a
  us-east1-b
  europe-west4-a

  us-west1-b
  us-central1-b
  us-east1-c
  europe-west4-b

  us-west1-c
  us-east1-d
  europe-west4-c
)

# GCE zones with enough quota for one CPU-only validator
GCE_LOW_QUOTA_ZONES=(
  asia-east2-a
  asia-northeast1-b
  asia-northeast2-b
  asia-south1-c
  asia-southeast1-b
  australia-southeast1-b
  europe-north1-a
  europe-west2-b
  europe-west3-c
  europe-west6-a
  northamerica-northeast1-a
  southamerica-east1-b
)

case $TESTNET in
testnet-edge)
  CHANNEL_OR_TAG=edge
  CHANNEL_BRANCH=$EDGE_CHANNEL
  ;;
testnet-beta)
  CHANNEL_OR_TAG=beta
  CHANNEL_BRANCH=$BETA_CHANNEL
  ;;
testnet)
  CHANNEL_OR_TAG=$STABLE_CHANNEL_LATEST_TAG
  CHANNEL_BRANCH=$STABLE_CHANNEL
  export CLOUDSDK_CORE_PROJECT=testnet-solana-com
  ;;
*)
  echo "Error: Invalid TESTNET=$TESTNET"
  exit 1
  ;;
esac

EC2_ZONE_ARGS=()
for val in "${EC2_ZONES[@]}"; do
  EC2_ZONE_ARGS+=("-z $val")
done
GCE_ZONE_ARGS=()
for val in "${GCE_ZONES[@]}"; do
  GCE_ZONE_ARGS+=("-z $val")
done
GCE_LOW_QUOTA_ZONE_ARGS=()
for val in "${GCE_LOW_QUOTA_ZONES[@]}"; do
  GCE_LOW_QUOTA_ZONE_ARGS+=("-z $val")
done

if [[ -z $TESTNET_DB_HOST ]]; then
  TESTNET_DB_HOST="https://metrics.solana.com:8086"
fi

export SOLANA_METRICS_CONFIG="db=$TESTNET,host=$TESTNET_DB_HOST,$SOLANA_METRICS_PARTIAL_CONFIG"
echo "SOLANA_METRICS_CONFIG: $SOLANA_METRICS_CONFIG"
source scripts/configure-metrics.sh

if [[ -n $TESTNET_TAG ]]; then
  CHANNEL_OR_TAG=$TESTNET_TAG
else

  if [[ $CI_BRANCH != "$CHANNEL_BRANCH" ]]; then
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
        TESTNET_DB_HOST: "$TESTNET_DB_HOST"
        GCE_NODE_COUNT: "$GCE_NODE_COUNT"
        GCE_LOW_QUOTA_NODE_COUNT: "$GCE_LOW_QUOTA_NODE_COUNT"
        RUST_LOG: "$RUST_LOG"
EOF
    ) | buildkite-agent pipeline upload
    exit 0
  fi
fi

maybe_deploy_software() {
  declare arg=$1
  declare ok=true
  (
    echo "--- net.sh restart"
    set -x
    time net/net.sh restart --skip-setup -t "$CHANNEL_OR_TAG" --skip-poh-verify "$arg"
  ) || ok=false
  if ! $ok; then
    net/net.sh logs
  fi
  $ok
}

sanity() {
  echo "--- sanity $TESTNET"
  case $TESTNET in
  testnet-edge)
    (
      set -x
      NO_INSTALL_CHECK=1 \
        ci/testnet-sanity.sh edge-devnet-solana-com gce -P us-west1-b
      maybe_deploy_software
    )
    ;;
  testnet-beta)
    (
      set -x
      NO_INSTALL_CHECK=1 \
        ci/testnet-sanity.sh beta-devnet-solana-com gce -P us-west1-b
      maybe_deploy_software --deploy-if-newer
    )
    ;;
  testnet)
    (
      set -x
      ci/testnet-sanity.sh devnet-solana-com gce -P us-west1-b
    )
    ;;
  *)
    echo "Error: Invalid TESTNET=$TESTNET"
    exit 1
    ;;
  esac
}

deploy() {
  declare maybeCreate=$1
  declare maybeStart=$2
  declare maybeStop=$3
  declare maybeDelete=$4

  echo "--- deploy \"$maybeCreate\" \"$maybeStart\" \"$maybeStop\" \"$maybeDelete\""

  # Create or recreate the nodes
  if [[ -z $maybeCreate ]]; then
    skipCreate=skip
  else
    skipCreate=""
  fi

  # Start or restart the network software on the nodes
  if [[ -z $maybeStart ]]; then
    skipStart=skip
  else
    skipStart=""
  fi

  case $TESTNET in
  testnet-edge)
    (
      set -x
      ci/testnet-deploy.sh -p edge-devnet-solana-com -C gce -z us-west1-b \
        -t "$CHANNEL_OR_TAG" -n 3 -c 0 -u -P \
        -a edge-devnet-solana-com --letsencrypt edge.devnet.solana.com \
        --limit-ledger-size \
        ${skipCreate:+-e} \
        ${skipStart:+-s} \
        ${maybeStop:+-S} \
        ${maybeDelete:+-D}
    )
    ;;
  testnet-beta)
    (
      set -x
      ci/testnet-deploy.sh -p beta-devnet-solana-com -C gce -z us-west1-b \
        -t "$CHANNEL_OR_TAG" -n 3 -c 0 -u -P \
        -a beta-devnet-solana-com --letsencrypt beta.devnet.solana.com \
        --limit-ledger-size \
        ${skipCreate:+-e} \
        ${skipStart:+-s} \
        ${maybeStop:+-S} \
        ${maybeDelete:+-D}
    )
    ;;
  testnet)
    (
      set -x
      ci/testnet-deploy.sh -p devnet-solana-com -C gce -z us-west1-b \
        -t "$CHANNEL_OR_TAG" -n 0 -c 0 -u -P \
        -a devnet-solana-com --letsencrypt devnet.solana.com \
        --limit-ledger-size \
        ${skipCreate:+-e} \
        ${skipStart:+-s} \
        ${maybeStop:+-S} \
        ${maybeDelete:+-D}
    )
    (
      echo "--- net.sh update"
      set -x
      time net/net.sh update -t "$CHANNEL_OR_TAG" --platform linux --platform osx #--platform windows
    )
    ;;
  *)
    echo "Error: Invalid TESTNET=$TESTNET"
    exit 1
    ;;
  esac
}

ENABLED_LOCKFILE="${HOME}/${TESTNET}.is_enabled"

create-and-start() {
  deploy create start
}
create() {
  deploy create
}
start() {
  deploy "" start
}
stop() {
  deploy "" ""
}
delete() {
  deploy "" "" "" delete
}
enable_testnet() {
  touch "${ENABLED_LOCKFILE}"
  echo "+++ $TESTNET now enabled"
}
disable_testnet() {
  rm -f "${ENABLED_LOCKFILE}"
  echo "+++ $TESTNET now disabled"
}
is_testnet_enabled() {
  if [[ ! -f ${ENABLED_LOCKFILE} ]]; then
    echo "+++ ${TESTNET} is currently disabled.  Enable ${TESTNET} by running ci/testnet-manager.sh with \$TESTNET_OP=enable, then re-run with current settings."
    exit 0
  fi
}

case $TESTNET_OP in
enable)
  enable_testnet
  ;;
disable)
  disable_testnet
  delete
  ;;
create-and-start)
  is_testnet_enabled
  create-and-start
  ;;
create)
  is_testnet_enabled
  create
  ;;
start)
  is_testnet_enabled
  start
  ;;
stop)
  is_testnet_enabled
  stop
  ;;
sanity)
  is_testnet_enabled
  sanity
  ;;
delete)
  is_testnet_enabled
  delete
  ;;
update-or-restart)
  is_testnet_enabled
  if start; then
    echo Update successful
  else
    echo "+++ Update failed, restarting the network"
    $metricsWriteDatapoint "testnet-manager update-failure=1"
    create-and-start
  fi
  ;;
sanity-or-restart)
  is_testnet_enabled
  if sanity; then
    echo Pass
  else
    echo "+++ Sanity failed, updating the network"
    $metricsWriteDatapoint "testnet-manager sanity-failure=1"
    create-and-start
  fi
  ;;
*)
  echo "Error: Invalid TESTNET_OP=$TESTNET_OP"
  exit 1
  ;;
esac

echo --- fin
exit 0
