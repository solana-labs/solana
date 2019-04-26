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
          - label: "testnet-perf"
            value: "testnet-perf"
          - label: "testnet-edge"
            value: "testnet-edge"
          - label: "testnet-edge-perf"
            value: "testnet-edge-perf"
          - label: "testnet-beta"
            value: "testnet-beta"
          - label: "testnet-beta-perf"
            value: "testnet-beta-perf"
          - label: "testnet-demo"
            value: "testnet-demo"
      - select: "Operation"
        key: "testnet-operation"
        default: "sanity-or-restart"
        options:
          - label: "Create new testnet nodes and then start network software.  If nodes are already created, they will be deleted and then re-created."
            value: "create-and-start"
          - label: "Create new testnet nodes, but do not start network software.  If nodes are already created, they will be deleted and then re-created."
            value: "create"
          - label: "Start network software on already-created testnet nodes.  If software is already running, it will be restarted."
            value: "start"
          - label: "Stop network software without deleting testnet nodes"
            value: "stop"
          - label: "Update the network software.  Restart network software on failure"
            value: "update-or-restart"
          - label: "Sanity check.  Restart network software on failure"
            value: "sanity-or-restart"
          - label: "Sanity check only"
            value: "sanity"
          - label: "Delete all nodes on a testnet.  Network software will be stopped first if it is running"
            value: "delete"
  - command: "ci/$(basename "$0")"
    agents:
      - "queue=$BUILDKITE_AGENT_META_DATA_QUEUE"
EOF
  ) | buildkite-agent pipeline upload
  exit 0
fi

ci/channel-info.sh
eval "$(ci/channel-info.sh)"


EC2_ZONES=(us-west-1a sa-east-1a ap-northeast-2a eu-central-1a ca-central-1a)
GCE_ZONES=(us-west1-b asia-east2-a europe-west4-a southamerica-east1-b us-east4-c)
case $TESTNET in
testnet-edge|testnet-edge-perf)
  CHANNEL_OR_TAG=edge
  CHANNEL_BRANCH=$EDGE_CHANNEL
  : "${TESTNET_DB_HOST:=https://clocktower-f1d56615.influxcloud.net:8086}"
  ;;
testnet-beta|testnet-beta-perf)
  CHANNEL_OR_TAG=beta
  CHANNEL_BRANCH=$BETA_CHANNEL
  : "${TESTNET_DB_HOST:=https://clocktower-f1d56615.influxcloud.net:8086}"
  : "${EC2_NODE_COUNT:=10}"
  : "${GCE_NODE_COUNT:=}"
  ;;
testnet|testnet-perf)
  CHANNEL_OR_TAG=$STABLE_CHANNEL_LATEST_TAG
  CHANNEL_BRANCH=$STABLE_CHANNEL
  : "${TESTNET_DB_HOST:=https://clocktower-f1d56615.influxcloud.net:8086}"
  ;;
testnet-demo)
  CHANNEL_OR_TAG=beta
  CHANNEL_BRANCH=$BETA_CHANNEL
  ;;
*)
  echo "Error: Invalid TESTNET=$TESTNET"
  exit 1
  ;;
esac

if [[ -n $TESTNET_DB_HOST ]]; then
  SOLANA_METRICS_PARTIAL_CONFIG="host=$TESTNET_DB_HOST,$SOLANA_METRICS_PARTIAL_CONFIG"
fi

export SOLANA_METRICS_CONFIG="db=$TESTNET,$SOLANA_METRICS_PARTIAL_CONFIG"
echo "SOLANA_METRICS_CONFIG: $SOLANA_METRICS_CONFIG"
source scripts/configure-metrics.sh

if [[ -n $TESTNET_TAG ]]; then
  CHANNEL_OR_TAG=$TESTNET_TAG
else

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
        TESTNET_DB_HOST: "$TESTNET_DB_HOST"
        EC2_NODE_COUNT: "$EC2_NODE_COUNT"
        GCE_NODE_COUNT: "$GCE_NODE_COUNT"
EOF
    ) | buildkite-agent pipeline upload
    exit 0
  fi
fi

sanity() {
  echo "--- sanity $TESTNET"
  case $TESTNET in
  testnet-edge)
    (
      set -x
      NO_LEDGER_VERIFY=1 \
        ci/testnet-sanity.sh edge-testnet-solana-com ec2 us-west-1a
    )
    ;;
  testnet-edge-perf)
    (
      set -x
      REJECT_EXTRA_NODES=1 \
      NO_LEDGER_VERIFY=1 \
      NO_VALIDATOR_SANITY=1 \
        ci/testnet-sanity.sh edge-perf-testnet-solana-com ec2 us-west-2b
    )
    ;;
  testnet-beta)
    (
      set -x

      ok=true
      if [[ -n $EC2_NODE_COUNT ]]; then
        NO_LEDGER_VERIFY=1 \
          ci/testnet-sanity.sh beta-testnet-solana-com ec2 "${EC2_ZONES[0]}" || ok=false
      elif [[ -n $GCE_NODE_COUNT ]]; then
        NO_LEDGER_VERIFY=1 \
          ci/testnet-sanity.sh beta-testnet-solana-com gce "${GCE_ZONES[0]}" || ok=false
      else
        echo "Error: no EC2 or GCE nodes"
        ok=false
      fi
      $ok
    )
    ;;
  testnet-beta-perf)
    (
      set -x
      REJECT_EXTRA_NODES=1 \
      NO_LEDGER_VERIFY=1 \
      NO_VALIDATOR_SANITY=1 \
        ci/testnet-sanity.sh beta-perf-testnet-solana-com ec2 us-west-2b
    )
    ;;
  testnet)
    (
      set -x
      NO_LEDGER_VERIFY=1 \
        ci/testnet-sanity.sh testnet-solana-com ec2 us-west-1a
      #ci/testnet-sanity.sh testnet-solana-com gce us-east1-c
    )
    ;;
  testnet-perf)
    (
      set -x
      REJECT_EXTRA_NODES=1 \
      NO_LEDGER_VERIFY=1 \
      NO_VALIDATOR_SANITY=1 \
        ci/testnet-sanity.sh perf-testnet-solana-com gce us-west1-b
      #ci/testnet-sanity.sh perf-testnet-solana-com ec2 us-east-1a
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

  # Create or recreate the nodes
  if [[ -z $maybeCreate ]]; then
    skipCreate=skip
  else
    skipCreate=""
    echo "--- create $TESTNET"
  fi

  # Start or restart the network software on the nodes
  if [[ -z $maybeStart ]]; then
    skipStart=skip
  else
    skipStart=""
    echo "--- start $TESTNET"
  fi

  # Stop the nodes
  if [[ -n $maybeStop ]]; then
    echo "--- stop $TESTNET"
  fi

  # Delete the nodes
  if [[ -n $maybeDelete ]]; then
    echo "--- delete $TESTNET"
  fi

  case $TESTNET in
  testnet-edge)
    (
      set -x
      ci/testnet-deploy.sh -p edge-testnet-solana-com -C ec2 -z us-west-1a \
        -t "$CHANNEL_OR_TAG" -n 3 -c 0 -u -P -a eipalloc-0ccd4f2239886fa94 \
        ${skipCreate:+-r} \
        ${skipStart:+-s} \
        ${maybeStop:+-S} \
        ${maybeDelete:+-D}
    )
    ;;
  testnet-edge-perf)
    (
      set -x
      NO_LEDGER_VERIFY=1 \
      NO_VALIDATOR_SANITY=1 \
      RUST_LOG=solana=warn \
        ci/testnet-deploy.sh -p edge-perf-testnet-solana-com -C ec2 -z us-west-2b \
          -g -t "$CHANNEL_OR_TAG" -c 2 \
          -b \
          ${skipCreate:+-r} \
          ${skipStart:+-s} \
          ${maybeStop:+-S} \
          ${maybeDelete:+-D}
    )
    ;;
  testnet-beta)
    (
      set -x

      # Build an array to pass as opts to testnet-deploy.sh: "-z zone1 -z zone2 ..."
      GCE_ZONE_ARGS=()
      for val in "${GCE_ZONES[@]}"; do
        GCE_ZONE_ARGS+=("-z $val")
      done

      EC2_ZONE_ARGS=()
      for val in "${EC2_ZONES[@]}"; do
        EC2_ZONE_ARGS+=("-z $val")
      done

      if [[ -n $EC2_NODE_COUNT ]]; then
        if [[ -n $GCE_NODE_COUNT ]] || [[ -n $skipStart ]]; then
          maybeSkipStart="skip"
        fi

        # shellcheck disable=SC2068
        ci/testnet-deploy.sh -p beta-testnet-solana-com -C ec2 ${EC2_ZONE_ARGS[@]} \
          -t "$CHANNEL_OR_TAG" -n "$EC2_NODE_COUNT" -c 0 -u -P -a eipalloc-0f286cf8a0771ce35 \
          ${skipCreate:+-r} \
          ${maybeSkipStart:+-s} \
          ${maybeStop:+-S} \
          ${maybeDelete:+-D}
      fi

      if [[ -n $GCE_NODE_COUNT ]]; then
        # shellcheck disable=SC2068
        ci/testnet-deploy.sh -p beta-testnet-solana-com -C gce ${GCE_ZONE_ARGS[@]} \
          -t "$CHANNEL_OR_TAG" -n "$GCE_NODE_COUNT" -c 0 -P \
          ${skipCreate:+-r} \
          ${skipStart:+-s} \
          ${maybeStop:+-S} \
          ${maybeDelete:+-D} \
          ${EC2_NODE_COUNT:+-x}
      fi
    )
    ;;
  testnet-beta-perf)
    (
      set -x
      NO_LEDGER_VERIFY=1 \
      NO_VALIDATOR_SANITY=1 \
      RUST_LOG=solana=warn \
        ci/testnet-deploy.sh -p beta-perf-testnet-solana-com -C ec2 -z us-west-2b \
          -g -t "$CHANNEL_OR_TAG" -c 2 \
          -b \
          ${skipCreate:+-r} \
          ${skipStart:+-s} \
          ${maybeStop:+-S} \
          ${maybeDelete:+-D}
    )
    ;;
  testnet)
    (
      set -x
      NO_VALIDATOR_SANITY=1 \
        ci/testnet-deploy.sh -p testnet-solana-com -C ec2 -z us-west-1a \
          -t "$CHANNEL_OR_TAG" -n 3 -c 0 -u -P -a eipalloc-0fa502bf95f6f18b2 \
          -b \
          ${skipCreate:+-r} \
          ${skipStart:+-s} \
          ${maybeStop:+-S} \
          ${maybeDelete:+-D}
        #ci/testnet-deploy.sh -p testnet-solana-com -C gce -z us-east1-c \
        #  -t "$CHANNEL_OR_TAG" -n 3 -c 0 -P -a testnet-solana-com  \
        #  ${maybeReuseLedger:+-r} \
        #  ${maybeDelete:+-D}
    )
    ;;
  testnet-perf)
    (
      set -x
      NO_LEDGER_VERIFY=1 \
      NO_VALIDATOR_SANITY=1 \
      RUST_LOG=solana=warn \
        ci/testnet-deploy.sh -p perf-testnet-solana-com -C gce -z us-west1-b \
          -G "--machine-type n1-standard-16 --accelerator count=2,type=nvidia-tesla-v100" \
          -t "$CHANNEL_OR_TAG" -c 2 \
          -b \
          -d pd-ssd \
          ${skipCreate:+-r} \
          ${skipStart:+-s} \
          ${maybeStop:+-S} \
          ${maybeDelete:+-D}
        #ci/testnet-deploy.sh -p perf-testnet-solana-com -C ec2 -z us-east-1a \
        #  -g \
        #  -t "$CHANNEL_OR_TAG" -c 2 \
        #  ${maybeReuseLedger:+-r} \
        #  ${maybeDelete:+-D}
    )
    ;;
  testnet-demo)
    (
      set -x
      echo "Demo net not yet implemented!"
      exit 1
    )
    ;;
  *)
    echo "Error: Invalid TESTNET=$TESTNET"
    exit 1
    ;;
  esac
}

CREATED_LOCKFILE="${HOME}/${TESTNET}.is_created"
STARTED_LOCKFILE="${HOME}/${TESTNET}.is_started"

create-and-start() {
  rm -f "${CREATED_LOCKFILE}"
  rm -f "${STARTED_LOCKFILE}"
  deploy create start
  touch "${CREATED_LOCKFILE}"
  touch "${STARTED_LOCKFILE}"
}
create() {
  rm -f "${CREATED_LOCKFILE}"
  rm -f "${STARTED_LOCKFILE}"
  deploy create
  touch "${CREATED_LOCKFILE}"
}
start() {
  if [[ -f ${CREATED_LOCKFILE} ]]; then
    rm -f "${STARTED_LOCKFILE}"
    deploy "" start
    touch "${STARTED_LOCKFILE}"
  else
    echo "Unable to start ${TESTNET}.  Are the nodes created?
    Re-run ci/testnet-manager.sh with \$TESTNET_OP=create or \$TESTNET_OP=create-and-start"
    exit 1
  fi
}
stop() {
  deploy "" ""
  rm -f "${STARTED_LOCKFILE}"
}
delete() {
  deploy "" "" "" delete
  rm -f "${CREATED_LOCKFILE}"
  rm -f "${STARTED_LOCKFILE}"
}

case $TESTNET_OP in
create-and-start)
  create-and-start
  ;;
create)
  create
  ;;
start)
  start
  ;;
stop)
  stop
  ;;
sanity)
  sanity
  ;;
delete)
  delete
  ;;
update-or-restart)
  if start; then
    echo Update successful
  else
    echo "+++ Update failed, restarting the network"
    $metricsWriteDatapoint "testnet-manager update-failure=1"
    start
  fi
  ;;
sanity-or-restart)
  if sanity; then
    echo Pass
  else
    echo "+++ Sanity failed, updating the network"
    $metricsWriteDatapoint "testnet-manager sanity-failure=1"

    # TODO: Restore attempt to restart the cluster before recreating it
    #       See https://github.com/solana-labs/solana/issues/3774
    if false; then
      if start; then
        echo Update successful
      else
        echo "+++ Update failed, restarting the network"
        $metricsWriteDatapoint "testnet-manager update-failure=1"
        start
      fi
    else
      start
    fi
  fi
  ;;
*)
  echo "Error: Invalid TESTNET_OP=$TESTNET_OP"
  exit 1
  ;;
esac

echo --- fin
exit 0
