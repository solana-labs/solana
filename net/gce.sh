#!/bin/bash -e

here=$(dirname "$0")
# shellcheck source=net/common.sh
source "$here"/common.sh

cloudProvider=$(basename "$0" .sh)
bootDiskType=""
case $cloudProvider in
gce)
  # shellcheck source=net/scripts/gce-provider.sh
  source "$here"/scripts/gce-provider.sh

  cpuLeaderMachineType=n1-standard-16
  gpuLeaderMachineType="$cpuLeaderMachineType --accelerator count=4,type=nvidia-tesla-k80"
  leaderMachineType=$cpuLeaderMachineType
  validatorMachineType=n1-standard-16
  clientMachineType=n1-standard-16
  ;;
ec2)
  # shellcheck source=net/scripts/ec2-provider.sh
  source "$here"/scripts/ec2-provider.sh

  cpuLeaderMachineType=m4.4xlarge
  gpuLeaderMachineType=p2.xlarge
  leaderMachineType=$cpuLeaderMachineType
  validatorMachineType=m4.2xlarge
  clientMachineType=m4.2xlarge
  ;;
*)
  echo "Error: Unknown cloud provider: $cloudProvider"
  ;;
esac


prefix=testnet-dev-${USER//[^A-Za-z0-9]/}
validatorNodeCount=5
clientNodeCount=1
leaderBootDiskSizeInGb=1000
validatorBootDiskSizeInGb=$leaderBootDiskSizeInGb
clientBootDiskSizeInGb=75

publicNetwork=false
enableGpu=false
leaderAddress=

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

 common options:
   -p [prefix]      - Optional common prefix for instance names to avoid
                      collisions (default: $prefix)

 create-specific options:
   -n [number]      - Number of validator nodes (default: $validatorNodeCount)
   -c [number]      - Number of client nodes (default: $clientNodeCount)
   -P               - Use public network IP addresses (default: $publicNetwork)
   -z [zone]        - Zone for the nodes (default: $zone)
   -g               - Enable GPU (default: $enableGpu)
   -G               - Enable GPU, and set count/type of GPUs to use (e.g $cpuLeaderMachineType --accelerator count=4,type=nvidia-tesla-k80)
   -a [address]     - Set the leader node's external IP address to this value.
                      For GCE, [address] is the "name" of the desired External
                      IP Address.
                      For EC2, [address] is the "allocation ID" of the desired
                      Elastic IP.
   -d [disk-type]   - Specify a boot disk type (default None) Use pd-ssd to get ssd on GCE.

 config-specific options:
   none

 delete-specific options:
   none

EOF
  exit $exitcode
}


command=$1
[[ -n $command ]] || usage
shift
[[ $command = create || $command = config || $command = delete ]] || usage "Invalid command: $command"

while getopts "h?p:Pn:c:z:gG:a:d:" opt; do
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
    validatorNodeCount=$OPTARG
    ;;
  c)
    clientNodeCount=$OPTARG
    ;;
  z)
    cloud_SetZone "$OPTARG"
    ;;
  g)
    enableGpu=true
    leaderMachineType=$gpuLeaderMachineType
    ;;
  G)
    enableGpu=true
    leaderMachineType="$OPTARG"
    ;;
  a)
    leaderAddress=$OPTARG
    ;;
  d)
    bootDiskType=$OPTARG
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
    # TODO: GPU image is still 16.04-based pending resolution of
    #       https://github.com/solana-labs/solana/issues/1702
    imageName="ubuntu-16-04-cuda-9-2-new"
  else
    imageName="ubuntu-1804-bionic-v20181029 --image-project ubuntu-os-cloud"
  fi
  ;;
ec2)
  # Deep Learning AMI (Ubuntu 16.04-based)
  case $region in # (region global variable is set by cloud_SetZone)
  us-east-1)
    imageName="ami-047daf3f2b162fc35"
    ;;
  us-west-1)
    imageName="ami-08c8c7c4a57a6106d"
    ;;
  us-west-2)
    imageName="ami-0b63040ee445728bf"
    ;;
  *)
    usage "Unsupported region: $region"
    ;;
  esac
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
EOF

  buildSshOptions

  recordInstanceIp() {
    declare name="$1"
    declare publicIp="$2"
    declare privateIp="$3"

    declare arrayName="$5"

    echo "$arrayName+=($publicIp)  # $name" >> "$configFile"
    if [[ $arrayName = "leaderIp" ]]; then
      if $publicNetwork; then
        echo "entrypointIp=$publicIp" >> "$configFile"
      else
        echo "entrypointIp=$privateIp" >> "$configFile"
      fi
    fi
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

  echo "Looking for leader instance..."
  cloud_FindInstance "$prefix-leader"
  [[ ${#instances[@]} -eq 1 ]] || {
    echo "Unable to find leader"
    exit 1
  }

  (
    declare leaderName
    declare leaderIp
    IFS=: read -r leaderName leaderIp _ < <(echo "${instances[0]}")

    # Try to ping the machine first.
    timeout 90s bash -c "set -o pipefail; until ping -c 3 $leaderIp | tr - _; do echo .; done"

    if [[ ! -r $sshPrivateKey ]]; then
      echo "Fetching $sshPrivateKey from $leaderName"

      # Try to scp in a couple times, sshd may not yet be up even though the
      # machine can be pinged...
      set -x -o pipefail
      for i in $(seq 1 30); do
        if cloud_FetchFile "$leaderName" "$leaderIp" /solana-id_ecdsa "$sshPrivateKey"; then
          break
        fi

        sleep 1
        echo "Retry $i..."
      done

      chmod 400 "$sshPrivateKey"
      ls -l "$sshPrivateKey"
    fi
  )

  echo "leaderIp=()" >> "$configFile"
  cloud_ForEachInstance recordInstanceIp leaderIp
  cloud_ForEachInstance waitForStartupComplete

  echo "Looking for validator instances..."
  cloud_FindInstances "$prefix-validator"
  [[ ${#instances[@]} -gt 0 ]] || {
    echo "Unable to find validators"
    exit 1
  }
  echo "validatorIpList=()" >> "$configFile"
  cloud_ForEachInstance recordInstanceIp validatorIpList
  cloud_ForEachInstance waitForStartupComplete

  echo "clientIpList=()" >> "$configFile"
  echo "Looking for client instances..."
  cloud_FindInstances "$prefix-client"
  [[ ${#instances[@]} -eq 0 ]] || {
    cloud_ForEachInstance recordInstanceIp clientIpList
    cloud_ForEachInstance waitForStartupComplete
  }

  echo "Wrote $configFile"
  $metricsWriteDatapoint "testnet-deploy net-config-complete=1"
}

delete() {
  $metricsWriteDatapoint "testnet-deploy net-delete-begin=1"

  # Delete the leader node first to prevent unusual metrics on the dashboard
  # during shutdown.
  # TODO: It would be better to fully cut-off metrics reporting before any
  # instances are deleted.
  for filter in "$prefix-leader" "$prefix-"; do
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
  [[ -n $validatorNodeCount ]] || usage "Need number of nodes"
  if [[ $validatorNodeCount -le 0 ]]; then
    usage "One or more validator nodes is required"
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
  Leader = $leaderMachineType (GPU=$enableGpu)
  Validators = $validatorNodeCount x $validatorMachineType
  Client(s) = $clientNodeCount x $clientMachineType

========================================================================================

EOF
  }
  printNetworkInfo

  declare startupScript="$netConfigDir"/instance-startup-script.sh
  cat > "$startupScript" <<EOF
#!/bin/bash -ex
# autogenerated at $(date)

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
    install-rsync.sh \
    network-config.sh \
    remove-docker-interface.sh \
    update-default-cuda.sh \

)

cat > /etc/motd <<EOM
$(printNetworkInfo)
EOM

touch /.instance-startup-complete

EOF

  cloud_CreateInstances "$prefix" "$prefix-leader" 1 \
    "$imageName" "$leaderMachineType" "$leaderBootDiskSizeInGb" \
    "$startupScript" "$leaderAddress" "$bootDiskType"

  cloud_CreateInstances "$prefix" "$prefix-validator" "$validatorNodeCount" \
    "$imageName" "$validatorMachineType" "$validatorBootDiskSizeInGb" \
    "$startupScript" "" "$bootDiskType"

  if [[ $clientNodeCount -gt 0 ]]; then
    cloud_CreateInstances "$prefix" "$prefix-client" "$clientNodeCount" \
      "$imageName" "$clientMachineType" "$clientBootDiskSizeInGb" \
      "$startupScript" "" "$bootDiskType"
  fi

  $metricsWriteDatapoint "testnet-deploy net-create-complete=1"

  prepareInstancesAndWriteConfigFile
  ;;

config)
  prepareInstancesAndWriteConfigFile
  ;;
*)
  usage "Unknown command: $command"
esac
