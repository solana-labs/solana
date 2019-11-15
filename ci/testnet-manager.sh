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
          - label: "tds"
            value: "tds"
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
testnet-edge|testnet-edge-perf)
  CHANNEL_OR_TAG=edge
  CHANNEL_BRANCH=$EDGE_CHANNEL
  ;;
testnet-beta|testnet-beta-perf)
  CHANNEL_OR_TAG=beta
  CHANNEL_BRANCH=$BETA_CHANNEL
  ;;
testnet)
  CHANNEL_OR_TAG=$STABLE_CHANNEL_LATEST_TAG
  CHANNEL_BRANCH=$STABLE_CHANNEL
  ;;
testnet-perf)
  CHANNEL_OR_TAG=$STABLE_CHANNEL_LATEST_TAG
  CHANNEL_BRANCH=$STABLE_CHANNEL
  ;;
testnet-demo)
  CHANNEL_OR_TAG=beta
  CHANNEL_BRANCH=$BETA_CHANNEL
  : "${GCE_NODE_COUNT:=150}"
  : "${GCE_LOW_QUOTA_NODE_COUNT:=70}"
  ;;
tds)
  : "${TDS_CHANNEL_OR_TAG:=edge}"
  CHANNEL_OR_TAG="$TDS_CHANNEL_OR_TAG"
  CHANNEL_BRANCH="$CI_BRANCH"
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
        ci/testnet-sanity.sh edge-testnet-solana-com gce -P us-west1-b
      maybe_deploy_software
    )
    ;;
  testnet-edge-perf)
    (
      set -x
      REJECT_EXTRA_NODES=1 \
        ci/testnet-sanity.sh edge-perf-testnet-solana-com ec2 us-west-2b
    )
    ;;
  testnet-beta)
    (
      set -x
      NO_INSTALL_CHECK=1 \
        ci/testnet-sanity.sh beta-testnet-solana-com gce us-west1-b
      maybe_deploy_software --deploy-if-newer
    )
    ;;
  testnet-beta-perf)
    (
      set -x
      REJECT_EXTRA_NODES=1 \
        ci/testnet-sanity.sh beta-perf-testnet-solana-com ec2 us-west-2b
    )
    ;;
  testnet)
    (
      set -x
      ci/testnet-sanity.sh testnet-solana-com gce us-west1-b
    )
    ;;
  testnet-perf)
    (
      set -x
      REJECT_EXTRA_NODES=1 \
        ci/testnet-sanity.sh perf-testnet-solana-com gce us-west1-b
      #ci/testnet-sanity.sh perf-testnet-solana-com ec2 us-east-1a
    )
    ;;
  testnet-demo)
    (
      set -x

      ok=true
      if [[ -n $GCE_NODE_COUNT ]]; then
        ci/testnet-sanity.sh demo-testnet-solana-com gce "${GCE_ZONES[0]}" -f || ok=false
      else
        echo "Error: no GCE nodes"
        ok=false
      fi
      $ok
    )
    ;;
  tds)
    (
      set -x
      ci/testnet-sanity.sh tds-solana-com gce "${GCE_ZONES[0]}" -f
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
      ci/testnet-deploy.sh -p edge-testnet-solana-com -C gce -z us-west1-b \
        -t "$CHANNEL_OR_TAG" -n 3 -c 0 -u -P \
        -a edge-testnet-solana-com --letsencrypt edge.testnet.solana.com \
        --limit-ledger-size \
        ${skipCreate:+-e} \
        ${skipStart:+-s} \
        ${maybeStop:+-S} \
        ${maybeDelete:+-D}
    )
    ;;
  testnet-edge-perf)
    (
      set -x
      RUST_LOG=solana=warn \
        ci/testnet-deploy.sh -p edge-perf-testnet-solana-com -C ec2 -z us-west-2b \
          -g -t "$CHANNEL_OR_TAG" -c 2 \
          ${skipCreate:+-e} \
          ${skipStart:+-s} \
          ${maybeStop:+-S} \
          ${maybeDelete:+-D}
    )
    ;;
  testnet-beta)
    (
      set -x
      ci/testnet-deploy.sh -p beta-testnet-solana-com -C gce -z us-west1-b \
        -t "$CHANNEL_OR_TAG" -n 3 -c 0 -u -P \
        -a beta-testnet-solana-com --letsencrypt beta.testnet.solana.com \
        --limit-ledger-size \
        ${skipCreate:+-e} \
        ${skipStart:+-s} \
        ${maybeStop:+-S} \
        ${maybeDelete:+-D}
    )
    ;;
  testnet-beta-perf)
    (
      set -x
      RUST_LOG=solana=warn \
        ci/testnet-deploy.sh -p beta-perf-testnet-solana-com -C ec2 -z us-west-2b \
          -g -t "$CHANNEL_OR_TAG" -c 2 \
          ${skipCreate:+-e} \
          ${skipStart:+-s} \
          ${maybeStop:+-S} \
          ${maybeDelete:+-D}
    )
    ;;
  testnet)
    (
      set -x
      ci/testnet-deploy.sh -p testnet-solana-com -C gce -z us-west1-b \
        -t "$CHANNEL_OR_TAG" -n 3 -c 0 -u -P \
        -a testnet-solana-com --letsencrypt testnet.solana.com \
        --limit-ledger-size \
        ${skipCreate:+-e} \
        ${skipStart:+-s} \
        ${maybeStop:+-S} \
        ${maybeDelete:+-D}
    )
    (
      echo "--- net.sh update"
      set -x
      time net/net.sh update -t edge --platform linux --platform osx --platform windows
    )
    ;;
  testnet-perf)
    (
      set -x
      RUST_LOG=solana=warn \
        ci/testnet-deploy.sh -p perf-testnet-solana-com -C gce -z us-west1-b \
          -G "--machine-type n1-standard-16 --accelerator count=2,type=nvidia-tesla-v100" \
          -t "$CHANNEL_OR_TAG" -c 2 \
          -d pd-ssd \
          ${skipCreate:+-e} \
          ${skipStart:+-s} \
          ${maybeStop:+-S} \
          ${maybeDelete:+-D}
    )
    ;;
  testnet-demo)
    (
      set -x

      if [[ -n $GCE_LOW_QUOTA_NODE_COUNT ]] || [[ -n $skipStart ]]; then
        maybeSkipStart="skip"
      fi

      # shellcheck disable=SC2068
      ci/testnet-deploy.sh -p demo-testnet-solana-com -C gce ${GCE_ZONE_ARGS[@]} \
        -t "$CHANNEL_OR_TAG" -n "$GCE_NODE_COUNT" -c 0 -P -u --allow-boot-failures \
        --skip-remote-log-retrieval \
        -a demo-testnet-solana-com \
        ${skipCreate:+-e} \
        ${maybeSkipStart:+-s} \
        ${maybeStop:+-S} \
        ${maybeDelete:+-D}

      if [[ -n $GCE_LOW_QUOTA_NODE_COUNT ]]; then
        # shellcheck disable=SC2068
        ci/testnet-deploy.sh -p demo-testnet-solana-com2 -C gce ${GCE_LOW_QUOTA_ZONE_ARGS[@]} \
          -t "$CHANNEL_OR_TAG" -n "$GCE_LOW_QUOTA_NODE_COUNT" -c 0 -P --allow-boot-failures -x \
          --skip-remote-log-retrieval \
          ${skipCreate:+-e} \
          ${skipStart:+-s} \
          ${maybeStop:+-S} \
          ${maybeDelete:+-D}
      fi
    )
    ;;
  tds)
    (
      set -x

      # Allow cluster configuration to be overridden from env vars

      if [[ -z $TDS_ZONES ]]; then
        TDS_ZONES="us-west1-a,us-central1-a,europe-west4-a"
      fi
      GCE_CLOUD_ZONES=(); while read -r -d, ; do GCE_CLOUD_ZONES+=( "$REPLY" ); done <<< "${TDS_ZONES},"

      if [[ -z $TDS_NODE_COUNT ]]; then
        TDS_NODE_COUNT="3"
      fi

      if [[ -z $TDS_CLIENT_COUNT ]]; then
        TDS_CLIENT_COUNT="1"
      fi

      if [[ -z $ENABLE_GPU ]]; then
        maybeGpu=(-G "--machine-type n1-standard-16 --accelerator count=2,type=nvidia-tesla-v100")
      elif [[ $ENABLE_GPU == skip ]]; then
        maybeGpu=()
      else
        maybeGpu=(-G "${ENABLE_GPU}")
      fi

      if [[ -z $HASHES_PER_TICK ]]; then
        maybeHashesPerTick="--hashes-per-tick auto"
      elif [[ $HASHES_PER_TICK == skip ]]; then
        maybeHashesPerTick=""
      else
        maybeHashesPerTick="--hashes-per-tick ${HASHES_PER_TICK}"
      fi

      if [[ -z $DISABLE_AIRDROPS ]]; then
        DISABLE_AIRDROPS="true"
      fi

      if [[ $DISABLE_AIRDROPS == true ]] ; then
        maybeDisableAirdrops="--no-airdrop"
      else
        maybeDisableAirdrops=""
      fi

      if [[ -z $INTERNAL_NODES_STAKE_LAMPORTS ]]; then
        maybeInternalNodesStakeLamports="--internal-nodes-stake-lamports 1000000000" # 1 SOL
      elif [[ $INTERNAL_NODES_STAKE_LAMPORTS == skip ]]; then
        maybeInternalNodesStakeLamports=""
      else
        maybeInternalNodesStakeLamports="--internal-nodes-stake-lamports ${INTERNAL_NODES_STAKE_LAMPORTS}"
      fi

      if [[ -z $INTERNAL_NODES_LAMPORTS ]]; then
        maybeInternalNodesLamports="--internal-nodes-lamports 2000000000" # 2 SOL
      elif [[ $INTERNAL_NODES_LAMPORTS == skip ]]; then
        maybeInternalNodesLamports=""
      else
        maybeInternalNodesLamports="--internal-nodes-lamports ${INTERNAL_NODES_LAMPORTS}"
      fi

      EXTERNAL_ACCOUNTS_FILE=/tmp/validator.yml
      if [[ -z $EXTERNAL_ACCOUNTS_FILE_URL ]]; then
        EXTERNAL_ACCOUNTS_FILE_URL=https://raw.githubusercontent.com/solana-labs/tour-de-sol/master/validators/all.yml
        wget ${EXTERNAL_ACCOUNTS_FILE_URL} -O ${EXTERNAL_ACCOUNTS_FILE}
        maybeExternalAccountsFile="--external-accounts-file ${EXTERNAL_ACCOUNTS_FILE}"
      elif [[ $EXTERNAL_ACCOUNTS_FILE_URL == skip ]]; then
        maybeExternalAccountsFile=""
      else
        wget ${EXTERNAL_ACCOUNTS_FILE_URL} -O ${EXTERNAL_ACCOUNTS_FILE}
        maybeExternalAccountsFile="--external-accounts-file ${EXTERNAL_ACCOUNTS_FILE}"
      fi

      if [[ -z $ADDITIONAL_DISK_SIZE_GB ]]; then
        maybeAdditionalDisk="--validator-additional-disk-size-gb 32000"
      elif [[ $ADDITIONAL_DISK_SIZE_GB == skip ]]; then
        maybeAdditionalDisk=""
      else
        maybeAdditionalDisk="--validator-additional-disk-size-gb ${ADDITIONAL_DISK_SIZE_GB}"
      fi

      # Multiple V100 GPUs are available in us-west1, us-central1 and europe-west4
      # shellcheck disable=SC2068
      # shellcheck disable=SC2086
      ci/testnet-deploy.sh -p tds-solana-com -C gce \
        "${maybeGpu[@]}" \
        -d pd-ssd \
        ${GCE_CLOUD_ZONES[@]/#/-z } \
        -t "$CHANNEL_OR_TAG" \
        -n ${TDS_NODE_COUNT} \
        -c ${TDS_CLIENT_COUNT} \
        --idle-clients \
        -P -u \
        -a tds-solana-com --letsencrypt tds.solana.com \
        ${maybeHashesPerTick} \
        ${skipCreate:+-e} \
        ${skipStart:+-s} \
        ${maybeStop:+-S} \
        ${maybeDelete:+-D} \
        ${maybeDisableAirdrops} \
        ${maybeInternalNodesStakeLamports} \
        ${maybeInternalNodesLamports} \
        ${maybeExternalAccountsFile} \
        --target-lamports-per-signature 0 \
        --slots-per-epoch 4096 \
        ${maybeAdditionalDisk}
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
