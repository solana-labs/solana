#!/usr/bin/env bash
set -e

cd "$(dirname "$0")"/../..

set -x
deployMethod="$1"
nodeType="$2"
entrypointIp="$3"
numNodes="$4"
if [[ -n $5 ]]; then
  export RUST_LOG="$5"
fi
skipSetup="$6"
failOnValidatorBootupFailure="$7"
externalPrimordialAccountsFile="$8"
maybeDisableAirdrops="$9"
internalNodesStakeLamports="${10}"
internalNodesLamports="${11}"
nodeIndex="${12}"
numBenchTpsClients="${13}"
benchTpsExtraArgs="${14}"
genesisOptions="${15}"
extraNodeArgs="${16}"
gpuMode="${17:-auto}"
maybeWarpSlot="${18}"
maybeFullRpc="${19}"
waitForNodeInit="${20}"
extraPrimordialStakes="${21:=0}"
tmpfsAccounts="${22:false}"
instanceIndex="${23}"
set +x

missing() {
  echo "Error: $1 not specified"
  exit 1
}

echo "greg - in remote-gossip-node.sh"

[[ -n $deployMethod ]]  || missing deployMethod
[[ -n $nodeType ]]      || missing nodeType
[[ -n $entrypointIp ]]  || missing entrypointIp
[[ -n $numNodes ]]      || missing numNodes
[[ -n $skipSetup ]]     || missing skipSetup
[[ -n $failOnValidatorBootupFailure ]] || missing failOnValidatorBootupFailure

airdropsEnabled=true
if [[ -n $maybeDisableAirdrops ]]; then
  airdropsEnabled=false
fi
cat > deployConfig <<EOF
deployMethod="$deployMethod"
entrypointIp="$entrypointIp"
numNodes="$numNodes"
failOnValidatorBootupFailure=$failOnValidatorBootupFailure
genesisOptions="$genesisOptions"
airdropsEnabled=$airdropsEnabled
EOF

source net/common.sh
source multinode-demo/common.sh
loadConfigFile

# initCompleteFile=init-complete-node.log

# cat > ~/solana/on-reboot <<EOF
# #!/usr/bin/env bash
# cd ~/solana
# source scripts/oom-score-adj.sh

# now=\$(date -u +"%Y-%m-%dT%H:%M:%SZ")
# ln -sfT validator.log.\$now validator.log
# EOF
# chmod +x ~/solana/on-reboot

  # rm -f ~/solana/gossip-only-write-keys
  # rm -f ~/solana/gossip-only-run

cat > ~/solana/gossip-only-run <<EOF
#!/usr/bin/env bash
cd ~/solana

now=\$(date -u +"%Y-%m-%dT%H:%M:%SZ")
ln -sfT validator.log.\$now validator.log
EOF
chmod +x ~/solana/gossip-only-run

cat > ~/solana/gossip-only-write-keys <<EOF
#!/usr/bin/env bash
cd ~/solana

EOF
chmod +x ~/solana/gossip-only-write-keys

# GPU_CUDA_OK=false
# GPU_FAIL_IF_NONE=false
# case "$gpuMode" in
#   on) # GPU *required*, any vendor
#     GPU_CUDA_OK=true
#     GPU_FAIL_IF_NONE=true
#     ;;
#   off) # CPU-only
#     ;;
#   auto) # Use GPU if installed, any vendor
#     GPU_CUDA_OK=true
#     ;;
#   cuda) # GPU *required*, CUDA-only
#     GPU_CUDA_OK=true
#     GPU_FAIL_IF_NONE=true
#     ;;
#   *)
#     echo "Unexpected gpuMode: \"$gpuMode\""
#     exit 1
#     ;;
# esac

case $deployMethod in
local|tar|skip)
  PATH="$HOME"/.cargo/bin:"$PATH"
  export USE_INSTALL=1

  # ./fetch-perf-libs.sh

# cat >> ~/solana/on-reboot <<EOF
#   PATH="$HOME"/.cargo/bin:"$PATH"
#   export USE_INSTALL=1

#   sudo RUST_LOG=info ~solana/.cargo/bin/solana-sys-tuner --user $(whoami) > sys-tuner.log 2>&1 &
#   echo \$! > sys-tuner.pid

#   (
#     sudo SOLANA_METRICS_CONFIG="$SOLANA_METRICS_CONFIG" scripts/oom-monitor.sh
#   ) > oom-monitor.log 2>&1 &
#   echo \$! > oom-monitor.pid
#   scripts/fd-monitor.sh > fd-monitor.log 2>&1 &
#   echo \$! > fd-monitor.pid
#   scripts/net-stats.sh  > net-stats.log 2>&1 &
#   echo \$! > net-stats.pid
#   scripts/iftop.sh  > iftop.log 2>&1 &
#   echo \$! > iftop.pid
#   scripts/system-stats.sh  > system-stats.log 2>&1 &
#   echo \$! > system-stats.pid

#   if ${GPU_CUDA_OK} && [[ -e /dev/nvidia0 ]]; then
#     echo Selecting solana-validator-cuda
#     export SOLANA_CUDA=1
#   elif ${GPU_FAIL_IF_NONE} ; then
#     echo "Expected GPU, found none!"
#     export SOLANA_GPU_MISSING=1
#   fi
# EOF

cat >> ~/solana/gossip-only-run <<EOF
  PATH="$HOME"/.cargo/bin:"$PATH"
  export USE_INSTALL=1

  sudo RUST_LOG=info ~solana/.cargo/bin/solana-sys-tuner --user $(whoami) > sys-tuner.log 2>&1 &
  echo \$! > sys-tuner.pid
EOF

cat >> ~/solana/gossip-only-write-keys <<EOF
  PATH="$HOME"/.cargo/bin:"$PATH"
  export USE_INSTALL=1

  sudo RUST_LOG=info ~solana/.cargo/bin/solana-sys-tuner --user $(whoami) > sys-tuner.log 2>&1 &
  echo \$! > sys-tuner.pid
EOF

  echo "greg - case - bootstrap or validator"


  case $nodeType in
  bootstrap-validator)
    echo "greg - in bootstrap-validator"
    set -x
    # if [[ $skipSetup != true ]]; then
    #   clear_config_dir "$SOLANA_CONFIG_DIR"

    #   if [[ -n $internalNodesLamports ]]; then
    #     echo "---" >> config/validator-balances.yml
    #   fi

    #   setupValidatorKeypair() {
    #     declare name=$1
    #     if [[ -f net/keypairs/"$name".json ]]; then
    #       cp net/keypairs/"$name".json config/"$name".json
    #       if [[ "$name" =~ ^validator-identity- ]]; then
    #         name="${name//-identity-/-vote-}"
    #         cp net/keypairs/"$name".json config/"$name".json
    #         name="${name//-vote-/-stake-}"
    #         cp net/keypairs/"$name".json config/"$name".json
    #       fi
    #     else
    #       solana-keygen new --no-passphrase -so config/"$name".json
    #       if [[ "$name" =~ ^validator-identity- ]]; then
    #         name="${name//-identity-/-vote-}"
    #         solana-keygen new --no-passphrase -so config/"$name".json
    #         name="${name//-vote-/-stake-}"
    #         solana-keygen new --no-passphrase -so config/"$name".json
    #       fi
    #     fi
    #   }
    # fi

    echo "greg - bootstrap - entrypoint IP: $entrypointIp"
    gossipOnlyPort=9001
    args=(
      --account-file gossip-only/src/accounts.yaml
      --bootstrap
      --num-nodes 1
      --entrypoint $entrypointIp:$gossipOnlyPort
      --gossip-host "$entrypointIp"
      --gossip-port $gossipOnlyPort
    )
    nohup gossip-only "${args[@]}" &> /dev/null &


    ;;
  validator|blockstreamer)

    if [[ $deployMethod != skip ]]; then
      net/scripts/rsync-retry.sh -vPrc "$entrypointIp":~/.cargo/bin/ ~/.cargo/bin/
      net/scripts/rsync-retry.sh -vPrc "$entrypointIp":~/version.yml ~/version.yml
    fi
    if [[ $skipSetup != true ]]; then
      clear_config_dir "$SOLANA_CONFIG_DIR"

      # if [[ $nodeType = blockstreamer ]]; then
      #   net/scripts/rsync-retry.sh -vPrc \
      #     "$entrypointIp":~/solana/config/blockstreamer-identity.json "$SOLANA_CONFIG_DIR"/validator-identity.json
      # else
      #   net/scripts/rsync-retry.sh -vPrc \
      #     "$entrypointIp":~/solana/config/validator-identity-"$nodeIndex".json "$SOLANA_CONFIG_DIR"/validator-identity.json
      #   net/scripts/rsync-retry.sh -vPrc \
      #     "$entrypointIp":~/solana/config/validator-stake-"$nodeIndex".json "$SOLANA_CONFIG_DIR"/stake-account.json
      #   net/scripts/rsync-retry.sh -vPrc \
      #     "$entrypointIp":~/solana/config/validator-vote-"$nodeIndex".json "$SOLANA_CONFIG_DIR"/vote-account.json
      # fi
      # net/scripts/rsync-retry.sh -vPrc \
      #   "$entrypointIp":~/solana/config/shred-version "$SOLANA_CONFIG_DIR"/shred-version

      # net/scripts/rsync-retry.sh -vPrc \
      #   "$entrypointIp":~/solana/config/bank-hash "$SOLANA_CONFIG_DIR"/bank-hash || true

      # net/scripts/rsync-retry.sh -vPrc \
      #   "$entrypointIp":~/solana/config/faucet.json "$SOLANA_CONFIG_DIR"/faucet.json
    fi

    echo "greg - running write keys - 2 "
    set -x
    echo "greg - validator - entrypoint IP: $entrypointIp"
    chmod +x gossip-only/src/gossip-only.sh

    args=(
      --account-file gossip-only/src/accounts.yaml
      --write-keys 
      --num-keys 1
    )

cat >> ~/solana/gossip-only-write-keys <<EOF
    gossip-only/src/gossip-only.sh ${args[@]} > gossip-instance-key-$instanceIndex.log 2>&1
EOF
    ~/solana/gossip-only-write-keys

    gossipOnlyPort=9001
    args=(
      --account-file gossip-only/src/accounts.yaml
      --num-nodes 1
      --entrypoint $entrypointIp:$gossipOnlyPort
      --gossip-host $(hostname -i)
    )

    echo "greg - instanceIndex: $instanceIndex"

cat >> ~/solana/gossip-only-run <<EOF
    nohup gossip-only/src/gossip-only.sh ${args[@]} >> gossip-instance-$instanceIndex.log.\$now 2>&1 &
    disown
EOF
    ~/solana/gossip-only-run

    ;;
  *)
    echo "Error: unknown node type: $nodeType"
    exit 1
    ;;
  esac
  ;;
*)
  echo "Unknown deployment method: $deployMethod"
  exit 1
esac
