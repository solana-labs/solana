#!/usr/bin/env bash
set -e

here=$(dirname "$0")
# shellcheck source=net/common.sh
source "$here"/common.sh

cloudProvider=$(basename "$0" .sh)
bootDiskType=""
case $cloudProvider in
gce)
  # shellcheck source=net/scripts/gce-provider.sh
  source "$here"/scripts/gce-provider.sh

  cpuBootstrapLeaderMachineType=n1-standard-16
  gpuBootstrapLeaderMachineType="$cpuBootstrapLeaderMachineType --accelerator count=4,type=nvidia-tesla-k80"
  bootstrapLeaderMachineType=$cpuBootstrapLeaderMachineType
  fullNodeMachineType=$cpuBootstrapLeaderMachineType
  clientMachineType=n1-standard-16
  blockstreamerMachineType=n1-standard-8
  ;;
ec2)
  # shellcheck source=net/scripts/ec2-provider.sh
  source "$here"/scripts/ec2-provider.sh

  cpuBootstrapLeaderMachineType=m4.4xlarge
  gpuBootstrapLeaderMachineType=p2.xlarge
  bootstrapLeaderMachineType=$cpuBootstrapLeaderMachineType
  fullNodeMachineType=m4.2xlarge
  clientMachineType=m4.2xlarge
  blockstreamerMachineType=m4.2xlarge
  ;;
*)
  echo "Error: Unknown cloud provider: $cloudProvider"
  ;;
esac


prefix=testnet-dev-${USER//[^A-Za-z0-9]/}
additionalFullNodeCount=5
clientNodeCount=1
blockstreamer=false
fullNodeBootDiskSizeInGb=1000
clientBootDiskSizeInGb=75

publicNetwork=false
enableGpu=false
customAddress=
leaderRotation=true

usage() {
  exitcode=0
  if [[ -n "$1" ]]; then
    exitcode=1
    echo "Error: $*"
  fi
  cat <<EOF
usage: $0 [create|config|delete] [common options] [command-specific options]

Manage testnet instances

 create - create a new testnet (implies 'config')
 config - configure the testnet and write a config file describing it
 delete - delete the testnet
 info   - display information about the currently configured testnet

 common options:
   -p [prefix]      - Optional common prefix for instance names to avoid
                      collisions (default: $prefix)
   -z [zone]        - Zone for the nodes (default: $zone)

 create-specific options:
   -n [number]      - Number of additional fullnodes (default: $additionalFullNodeCount)
   -c [number]      - Number of client nodes (default: $clientNodeCount)
   -u               - Include a Blockstreamer (default: $blockstreamer)
   -P               - Use public network IP addresses (default: $publicNetwork)
   -g               - Enable GPU (default: $enableGpu)
   -G               - Enable GPU, and set count/type of GPUs to use
                      (e.g $cpuBootstrapLeaderMachineType --accelerator count=4,type=nvidia-tesla-k80)
   -a [address]     - Address to be be assigned to the Blockstreamer if present,
                      otherwise the bootstrap fullnode.
                      * For GCE, [address] is the "name" of the desired External
                        IP Address.
                      * For EC2, [address] is the "allocation ID" of the desired
                        Elastic IP.
   -d [disk-type]   - Specify a boot disk type (default None) Use pd-ssd to get ssd on GCE.
   -b               - Disable leader rotation

 config-specific options:
   -P               - Use public network IP addresses (default: $publicNetwork)

 delete-specific options:
   none

 info-specific options:
   none

EOF
  exit $exitcode
}


command=$1
[[ -n $command ]] || usage
shift
[[ $command = create || $command = config || $command = info || $command = delete ]] ||
  usage "Invalid command: $command"

while getopts "h?p:Pn:c:z:gG:a:d:bu" opt; do
  case $opt in
  h | \?)
    usage
    ;;
  p)
    [[ ${OPTARG//[^A-Za-z0-9-]/} == "$OPTARG" ]] || usage "Invalid prefix: \"$OPTARG\", alphanumeric only"
    prefix=$OPTARG
    ;;
  P)
    publicNetwork=true
    ;;
  n)
    additionalFullNodeCount=$OPTARG
    ;;
  c)
    clientNodeCount=$OPTARG
    ;;
  z)
    cloud_SetZone "$OPTARG"
    ;;
  b)
    leaderRotation=false
    ;;
  g)
    enableGpu=true
    bootstrapLeaderMachineType=$gpuBootstrapLeaderMachineType
    fullNodeMachineType=$bootstrapLeaderMachineType
    ;;
  G)
    enableGpu=true
    bootstrapLeaderMachineType="$OPTARG"
    fullNodeMachineType=$bootstrapLeaderMachineType
    ;;
  a)
    customAddress=$OPTARG
    ;;
  d)
    bootDiskType=$OPTARG
    ;;
  u)
    blockstreamer=true
    ;;
  *)
    usage "unhandled option: $opt"
    ;;
  esac
done
shift $((OPTIND - 1))

[[ -z $1 ]] || usage "Unexpected argument: $1"
if [[ $cloudProvider = ec2 ]]; then
  # EC2 keys can't be retrieved from running instances like GCE keys can so save
  # EC2 keys in the user's home directory so |./ec2.sh config| can at least be
  # used on the same host that ran |./ec2.sh create| .
  sshPrivateKey="$HOME/.ssh/solana-net-id_$prefix"
else
  sshPrivateKey="$netConfigDir/id_$prefix"
fi

case $cloudProvider in
gce)
  if $enableGpu; then
    # Custom Ubuntu 18.04 LTS image with CUDA 9.2 and CUDA 10.0 installed
    #
    # TODO: Unfortunately this image is not public.  When this becomes an issue,
    # use the stock Ubuntu 18.04 image and programmatically install CUDA after the
    # instance boots
    #
    imageName="ubuntu-1804-bionic-v20181029-with-cuda-10-and-cuda-9-2"
  else
    # Upstream Ubuntu 18.04 LTS image
    imageName="ubuntu-1804-bionic-v20181029 --image-project ubuntu-os-cloud"
  fi
  ;;
ec2)
  if $enableGpu; then
    #
    # Custom Ubuntu 18.04 LTS image with CUDA 9.2 and CUDA 10.0 installed
    #
    # TODO: Unfortunately these AMIs are not public.  When this becomes an issue,
    # use the stock Ubuntu 18.04 image and programmatically install CUDA after the
    # instance boots
    #
    case $region in
    us-east-1)
      imageName="ami-0a8bd6fb204473f78"
      ;;
    us-west-1)
      imageName="ami-07011f0795513c59d"
      ;;
    us-west-2)
      imageName="ami-0a11ef42b62b82b68"
      ;;
    *)
      usage "Unsupported region: $region"
      ;;
    esac
  else
    # Select an upstream Ubuntu 18.04 AMI from https://cloud-images.ubuntu.com/locator/ec2/
    case $region in
    us-east-1)
      imageName="ami-0a313d6098716f372"
      ;;
    us-west-1)
      imageName="ami-06397100adf427136"
      ;;
    us-west-2)
      imageName="ami-0dc34f4b016c9ce49"
      ;;
    *)
      usage "Unsupported region: $region"
      ;;
    esac
  fi
  ;;
*)
  echo "Error: Unknown cloud provider: $cloudProvider"
  ;;
esac

# cloud_ForEachInstance [cmd] [extra args to cmd]
#
# Execute a command for each element in the `instances` array
#
#   cmd   - The command to execute on each instance
#           The command will receive arguments followed by any
#           additionl arguments supplied to cloud_ForEachInstance:
#               name     - name of the instance
#               publicIp - The public IP address of this instance
#               privateIp - The priate IP address of this instance
#               count    - Monotonically increasing count for each
#                          invocation of cmd, starting at 1
#               ...      - Extra args to cmd..
#
#
cloud_ForEachInstance() {
  declare cmd="$1"
  shift
  [[ -n $cmd ]] || { echo cloud_ForEachInstance: cmd not specified; exit 1; }

  declare count=1
  for info in "${instances[@]}"; do
    declare name publicIp privateIp
    IFS=: read -r name publicIp privateIp < <(echo "$info")

    eval "$cmd" "$name" "$publicIp" "$privateIp" "$count" "$@"
    count=$((count + 1))
  done
}

prepareInstancesAndWriteConfigFile() {
  $metricsWriteDatapoint "testnet-deploy net-config-begin=1"

  cat >> "$configFile" <<EOF
# autogenerated at $(date)
netBasename=$prefix
publicNetwork=$publicNetwork
sshPrivateKey=$sshPrivateKey
leaderRotation=$leaderRotation
EOF

  buildSshOptions

  recordInstanceIp() {
    declare name="$1"
    declare publicIp="$2"
    declare privateIp="$3"

    declare arrayName="$5"

    echo "$arrayName+=($publicIp)  # $name" >> "$configFile"
    echo "${arrayName}Private+=($privateIp)  # $name" >> "$configFile"
  }

  waitForStartupComplete() {
    declare name="$1"
    declare publicIp="$2"

    echo "Waiting for $name to finish booting..."
    (
      set -x +e
      for i in $(seq 1 60); do
        timeout 20s ssh "${sshOptions[@]}" "$publicIp" "ls -l /.instance-startup-complete"
        ret=$?
        if [[ $ret -eq 0 ]]; then
          exit 0
        fi
        sleep 2
        echo "Retry $i..."
      done
      echo "$name failed to boot."
      exit 1
    )
    echo "$name has booted."
  }

  echo "Looking for bootstrap leader instance..."
  cloud_FindInstance "$prefix-bootstrap-leader"
  [[ ${#instances[@]} -eq 1 ]] || {
    echo "Unable to find bootstrap leader"
    exit 1
  }

  (
    declare nodeName
    declare nodeIp
    IFS=: read -r nodeName nodeIp _ < <(echo "${instances[0]}")

    # Try to ping the machine first.
    timeout 90s bash -c "set -o pipefail; until ping -c 3 $nodeIp | tr - _; do echo .; done"

    if [[ ! -r $sshPrivateKey ]]; then
      echo "Fetching $sshPrivateKey from $nodeName"

      # Try to scp in a couple times, sshd may not yet be up even though the
      # machine can be pinged...
      set -x -o pipefail
      for i in $(seq 1 30); do
        if cloud_FetchFile "$nodeName" "$nodeIp" /solana-id_ecdsa "$sshPrivateKey"; then
          break
        fi

        sleep 1
        echo "Retry $i..."
      done

      chmod 400 "$sshPrivateKey"
      ls -l "$sshPrivateKey"
    fi
  )

  echo "fullnodeIpList=()" >> "$configFile"
  echo "fullnodeIpListPrivate=()" >> "$configFile"
  cloud_ForEachInstance recordInstanceIp fullnodeIpList
  cloud_ForEachInstance waitForStartupComplete

  echo "Looking for additional fullnode instances..."
  cloud_FindInstances "$prefix-fullnode"
  [[ ${#instances[@]} -gt 0 ]] || {
    echo "Unable to find additional fullnodes"
    exit 1
  }
  cloud_ForEachInstance recordInstanceIp fullnodeIpList
  cloud_ForEachInstance waitForStartupComplete

  echo "clientIpList=()" >> "$configFile"
  echo "clientIpListPrivate=()" >> "$configFile"
  echo "Looking for client bencher instances..."
  cloud_FindInstances "$prefix-client"
  [[ ${#instances[@]} -eq 0 ]] || {
    cloud_ForEachInstance recordInstanceIp clientIpList
    cloud_ForEachInstance waitForStartupComplete
  }

  echo "blockstreamerIpList=()" >> "$configFile"
  echo "blockstreamerIpListPrivate=()" >> "$configFile"
  echo "Looking for blockstreamer instances..."
  cloud_FindInstances "$prefix-blockstreamer"
  [[ ${#instances[@]} -eq 0 ]] || {
    cloud_ForEachInstance recordInstanceIp blockstreamerIpList
    cloud_ForEachInstance waitForStartupComplete
  }

  echo "Wrote $configFile"
  $metricsWriteDatapoint "testnet-deploy net-config-complete=1"
}

delete() {
  $metricsWriteDatapoint "testnet-deploy net-delete-begin=1"

  # Delete the bootstrap leader first to prevent unusual metrics on the dashboard
  # during shutdown (only applicable when leader rotation is disabled).
  # TODO: It would be better to fully cut-off metrics reporting before any
  # instances are deleted.
  for filter in "$prefix-bootstrap-leader" "$prefix-"; do
    echo "Searching for instances: $filter"
    cloud_FindInstances "$filter"

    if [[ ${#instances[@]} -eq 0 ]]; then
      echo "No instances found matching '$filter'"
    else
      cloud_DeleteInstances true
    fi
  done
  rm -f "$configFile"

  $metricsWriteDatapoint "testnet-deploy net-delete-complete=1"

}

case $command in
delete)
  delete
  ;;

create)
  [[ -n $additionalFullNodeCount ]] || usage "Need number of nodes"
  if [[ $additionalFullNodeCount -le 0 ]]; then
    usage "One or more additional fullnodes are required"
  fi

  delete

  $metricsWriteDatapoint "testnet-deploy net-create-begin=1"

  rm -rf "$sshPrivateKey"{,.pub}

  # Note: using rsa because |aws ec2 import-key-pair| seems to fail for ecdsa
  ssh-keygen -t rsa -N '' -f "$sshPrivateKey"

  printNetworkInfo() {
    cat <<EOF
========================================================================================

Network composition:
  Bootstrap leader = $bootstrapLeaderMachineType (GPU=$enableGpu)
  Additional fullnodes = $additionalFullNodeCount x $fullNodeMachineType
  Client(s) = $clientNodeCount x $clientMachineType
  Blockstreamer = $blockstreamer

Leader rotation: $leaderRotation

========================================================================================

EOF
  }
  printNetworkInfo

  declare startupScript="$netConfigDir"/instance-startup-script.sh
  cat > "$startupScript" <<EOF
#!/usr/bin/env bash
# autogenerated at $(date)
set -ex

cat > /etc/motd <<EOM
!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!

  This instance has not been fully configured.

  See startup script log messages in /var/log/syslog for status:
    $ sudo cat /var/log/syslog | egrep \\(startup-script\\|cloud-init\)

  To block until setup is complete, run:
    $ until [[ -f /.instance-startup-complete ]]; do sleep 1; done

!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!
EOM

# Place the generated private key at /solana-id_ecdsa so it's retrievable by anybody
# who is able to log into this machine
cat > /solana-id_ecdsa <<EOK
$(cat "$sshPrivateKey")
EOK
cat > /solana-id_ecdsa.pub <<EOK
$(cat "$sshPrivateKey.pub")
EOK
chmod 444 /solana-id_ecdsa

USER=\$(id -un)

$(
  cd "$here"/scripts/
  cat \
    disable-background-upgrades.sh \
    create-solana-user.sh \
    add-solana-user-authorized_keys.sh \
    install-earlyoom.sh \
    install-libssl-compatability.sh \
    install-nodejs.sh \
    install-redis.sh \
    install-rsync.sh \
    network-config.sh \
    remove-docker-interface.sh \

    if "$enableGpu"; then
      cat enable-nvidia-persistence-mode.sh
    fi

)

cat > /etc/motd <<EOM
$(printNetworkInfo)
EOM

touch /.instance-startup-complete

EOF

  if $blockstreamer; then
    blockstreamerAddress=$customAddress
  else
    bootstrapLeaderAddress=$customAddress
  fi

  cloud_Initialize "$prefix"

  cloud_CreateInstances "$prefix" "$prefix-bootstrap-leader" 1 \
    "$imageName" "$bootstrapLeaderMachineType" "$fullNodeBootDiskSizeInGb" \
    "$startupScript" "$bootstrapLeaderAddress" "$bootDiskType"

  cloud_CreateInstances "$prefix" "$prefix-fullnode" "$additionalFullNodeCount" \
    "$imageName" "$fullNodeMachineType" "$fullNodeBootDiskSizeInGb" \
    "$startupScript" "" "$bootDiskType"

  if [[ $clientNodeCount -gt 0 ]]; then
    cloud_CreateInstances "$prefix" "$prefix-client" "$clientNodeCount" \
      "$imageName" "$clientMachineType" "$clientBootDiskSizeInGb" \
      "$startupScript" "" "$bootDiskType"
  fi

  if $blockstreamer; then
    cloud_CreateInstances "$prefix" "$prefix-blockstreamer" "1" \
      "$imageName" "$blockstreamerMachineType" "$fullNodeBootDiskSizeInGb" \
      "$startupScript" "$blockstreamerAddress" "$bootDiskType"
  fi

  $metricsWriteDatapoint "testnet-deploy net-create-complete=1"

  prepareInstancesAndWriteConfigFile
  ;;

config)
  prepareInstancesAndWriteConfigFile
  ;;
info)
  loadConfigFile
  printNode() {
    declare nodeType=$1
    declare ip=$2
    declare ipPrivate=$3
    printf "  %-16s | %-15s | %-15s\n" "$nodeType" "$ip" "$ipPrivate"
  }

  printNode "Node Type" "Public IP" "Private IP"
  echo "-------------------+-----------------+-----------------"
  nodeType=bootstrap-leader
  for i in $(seq 0 $(( ${#fullnodeIpList[@]} - 1)) ); do
    ipAddress=${fullnodeIpList[$i]}
    ipAddressPrivate=${fullnodeIpListPrivate[$i]}
    printNode $nodeType "$ipAddress" "$ipAddressPrivate"
    nodeType=fullnode
  done

  for i in $(seq 0 $(( ${#clientIpList[@]} - 1)) ); do
    ipAddress=${clientIpList[$i]}
    ipAddressPrivate=${clientIpListPrivate[$i]}
    printNode bench-tps "$ipAddress" "$ipAddressPrivate"
  done

  for i in $(seq 0 $(( ${#blockstreamerIpList[@]} - 1)) ); do
    ipAddress=${blockstreamerIpList[$i]}
    ipAddressPrivate=${blockstreamerIpListPrivate[$i]}
    printNode blockstreamer "$ipAddress" "$ipAddressPrivate"
  done
  ;;
*)
  usage "Unknown command: $command"
esac
