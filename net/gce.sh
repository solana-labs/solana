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

  # use n1 instead of n2 so we don't need to spin up >= 4 local SSD's
  cpuBootstrapLeaderMachineType="--custom-cpu 24 --min-cpu-platform Intel%20Skylake --custom-vm-type n1"
  gpuBootstrapLeaderMachineType="$cpuBootstrapLeaderMachineType --accelerator count=1,type=nvidia-tesla-p100"
  clientMachineType="--custom-cpu 16 --custom-memory 20GB"
  blockstreamerMachineType="--machine-type n1-standard-8"
  selfDestructHours=8
  ;;
ec2)
  # shellcheck source=net/scripts/ec2-provider.sh
  source "$here"/scripts/ec2-provider.sh

  cpuBootstrapLeaderMachineType=m5.4xlarge

  # NOTE: At this time only the p3dn.24xlarge EC2 instance type has GPU and
  #       AVX-512 support.  The default, p2.xlarge, does not support
  #       AVX-512
  gpuBootstrapLeaderMachineType=p2.xlarge
  clientMachineType=c5.2xlarge
  blockstreamerMachineType=m5.4xlarge
  selfDestructHours=0
  ;;
azure)
  # shellcheck source=net/scripts/azure-provider.sh
  source "$here"/scripts/azure-provider.sh

  cpuBootstrapLeaderMachineType=Standard_D16s_v3
  gpuBootstrapLeaderMachineType=Standard_NC12
  clientMachineType=Standard_D16s_v3
  blockstreamerMachineType=Standard_D16s_v3
  selfDestructHours=0
  ;;
colo)
  # shellcheck source=net/scripts/colo-provider.sh
  source "$here"/scripts/colo-provider.sh

  cpuBootstrapLeaderMachineType=0
  gpuBootstrapLeaderMachineType=1
  clientMachineType=0
  blockstreamerMachineType=0
  selfDestructHours=0
  ;;
*)
  echo "Error: Unknown cloud provider: $cloudProvider"
  ;;
esac

prefix=testnet-dev-${USER//[^A-Za-z0-9]/}
additionalValidatorCount=2
clientNodeCount=0
blockstreamer=false
validatorBootDiskSizeInGb=500
clientBootDiskSizeInGb=75
validatorAdditionalDiskSizeInGb=
externalNodes=false
failOnValidatorBootupFailure=true
preemptible=true
evalInfo=false
tmpfsAccounts=false
defaultCustomMemoryGB="$(cloud_DefaultCustomMemoryGB)"
customMemoryGB="$defaultCustomMemoryGB"

publicNetwork=false
letsEncryptDomainName=
enableGpu=false
customMachineType=
customAddress=
zones=()

containsZone() {
  local e match="$1"
  shift
  for e; do [[ "$e" == "$match" ]] && return 0; done
  return 1
}

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
 status - display status information of all resources

 common options:
   -p [prefix]      - Optional common prefix for instance names to avoid
                      collisions (default: $prefix)
   -z [zone]        - Zone(s) for the nodes (default: $(cloud_DefaultZone))
                      If specified multiple times, the validators will be evenly
                      distributed over all specified zones and
                      client/blockstreamer nodes will be created in the first
                      zone
   -x               - append to the existing configuration instead of creating a
                      new configuration
   --allow-boot-failures
                    - Discard from config validator nodes that didn't bootup
                      successfully

 create-specific options:
   -n [number]      - Number of additional validators (default: $additionalValidatorCount)
   -c [number]      - Number of client nodes (default: $clientNodeCount)
   -u               - Include a Blockstreamer (default: $blockstreamer)
   -P               - Use public network IP addresses (default: $publicNetwork)
   -g               - Enable GPU and automatically set validator machine types to $gpuBootstrapLeaderMachineType
                      (default: $enableGpu)
   -G               - Enable GPU, and set custom GPU machine type to use
                      (e.g $gpuBootstrapLeaderMachineType)
   -a [address]     - Address to be be assigned to the Blockstreamer if present,
                      otherwise the bootstrap validator.
                      * For GCE, [address] is the "name" of the desired External
                        IP Address.
                      * For EC2, [address] is the "allocation ID" of the desired
                        Elastic IP.
   -d [disk-type]   - Specify a boot disk type (default None) Use pd-ssd to get ssd on GCE.
   --letsencrypt [dns name]
                    - Attempt to generate a TLS certificate using this
                      DNS name (useful only when the -a and -P options
                      are also provided)
   --custom-machine-type [type]
                    - Set a custom machine type without assuming whether or not
                      GPU is enabled.  Set this explicitly with --enable-gpu/-g to call out the presence of GPUs.
$(
  if [[ -n "$defaultCustomMemoryGB" ]]; then
    echo "   --custom-memory-gb"
    echo "                    - Set memory size for custom machine type in GB (default: $defaultCustomMemoryGB)"
  fi
)
   --enable-gpu     - Use with --custom-machine-type to specify whether or not GPUs should be used/enabled
   --validator-additional-disk-size-gb [number]
                    - Add an additional [number] GB SSD to all validators to store the config directory.
                      If not set, config will be written to the boot disk by default.
                      Only supported on GCE.
   --dedicated      - Use dedicated instances for additional validators
                      (by default preemptible instances are used to reduce
                      cost).  Note that the bootstrap validator,
                      blockstreamer and client nodes are always dedicated.
                      Set this flag on colo to prevent your testnet from being pre-empted by nightly test automation.
   --self-destruct-hours [number]
                    - Specify lifetime of the allocated instances in hours. 0 to
                      disable. Only supported on GCE. (default: $selfDestructHours)
   --validator-boot-disk-size-gb [number]
                    - Specify validator boot disk size in gb.
   --client-machine-type [type]
                    - custom client machine type
   --tmpfs-accounts - Put accounts directory on a swap-backed tmpfs volume

 config-specific options:
   -P               - Use public network IP addresses (default: $publicNetwork)

 delete-specific options:
   --reclaim-preemptible-reservations
                    - If set, reclaims all reservations on colo nodes that were not created with --dedicated.
                      This behavior does not filter by testnet name or owner.  Only implemented on colo.
   --reclaim-all-reservations
                    - If set, reclaims all reservations on all colo nodes, regardless of owner, pre-emptibility, or creator.

 info-specific options:
   --eval           - Output in a form that can be eval-ed by a shell: eval \$(gce.sh info --eval)

   none

EOF
  exit $exitcode
}

command=$1
[[ -n $command ]] || usage
shift
[[ $command = create || $command = config || $command = info || $command = delete || $command = status ]] ||
  usage "Invalid command: $command"

shortArgs=()
while [[ -n $1 ]]; do
  if [[ ${1:0:2} = -- ]]; then
    if [[ $1 = --letsencrypt ]]; then
      letsEncryptDomainName="$2"
      shift 2
    elif [[ $1 = --validator-additional-disk-size-gb ]]; then
      validatorAdditionalDiskSizeInGb="$2"
      shift 2
    elif [[ $1 == --machine-type* || $1 == --custom-cpu* ]]; then # Bypass quoted long args for GPUs
      shortArgs+=("$1")
      shift
    elif [[ $1 == --allow-boot-failures ]]; then
      failOnValidatorBootupFailure=false
      shift
    elif [[ $1 == --dedicated ]]; then
      preemptible=false
      shift
    elif [[ $1 == --eval ]]; then
      evalInfo=true
      shift
    elif [[ $1 == --enable-gpu ]]; then
      enableGpu=true
      shift
    elif [[ $1 = --custom-machine-type ]]; then
      customMachineType="$2"
      shift 2
    elif [[ $1 = --client-machine-type ]]; then
      clientMachineType="$2"
      shift 2
    elif [[ $1 = --validator-boot-disk-size-gb ]]; then
      validatorBootDiskSizeInGb="$2"
      shift 2
    elif [[ $1 == --self-destruct-hours ]]; then
      maybeTimeout=$2
      if [[ $maybeTimeout =~ ^[0-9]+$ ]]; then
        selfDestructHours=$maybeTimeout
      else
        echo "  Invalid parameter ($maybeTimeout) to $1"
        usage 1
      fi
      shift 2
    elif [[ $1 == --reclaim-preemptible-reservations ]]; then
      reclaimOnlyPreemptibleReservations=true
      shift
    elif [[ $1 == --reclaim-all-reservations ]]; then
      reclaimAllReservations=true
      shift
    elif [[ $1 == --tmpfs-accounts ]]; then
      tmpfsAccounts=true
      shift
    elif [[ $1 == --custom-memory-gb ]]; then
      customMemoryGB=$2
      shift 2
    else
      usage "Unknown long option: $1"
    fi
  else
    shortArgs+=("$1")
    shift
  fi
done

while getopts "h?p:Pn:c:r:z:gG:a:d:uxf" opt "${shortArgs[@]}"; do
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
    additionalValidatorCount=$OPTARG
    ;;
  c)
    clientNodeCount=$OPTARG
    ;;
  z)
    containsZone "$OPTARG" "${zones[@]}" || zones+=("$OPTARG")
    ;;
  g)
    enableGpu=true
    ;;
  G)
    enableGpu=true
    customMachineType="$OPTARG"
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
  x)
    externalNodes=true
    ;;
  *)
    usage "unhandled option: $opt"
    ;;
  esac
done

[[ ${#zones[@]} -gt 0 ]] || zones+=("$(cloud_DefaultZone)")

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
  if [[ "$tmpfsAccounts" = "true" ]]; then
    cpuBootstrapLeaderMachineType+=" --local-ssd interface=nvme"
    gpuBootstrapLeaderMachineType+=" --local-ssd interface=nvme"
    if [[ $customMemoryGB -lt 100 ]]; then
      # shellcheck disable=SC2016 # We don't want expression expansion on these backticks
      echo -e '\nWarning: At least 100GB of system RAM is recommending with `--tmpfs-accounts` (see `--custom-memory-gb`)\n'
    fi
  fi
  cpuBootstrapLeaderMachineType+=" --custom-memory ${customMemoryGB}GB"
  gpuBootstrapLeaderMachineType+=" --custom-memory ${customMemoryGB}GB"
  ;;
ec2|azure|colo)
  if [[ -n $validatorAdditionalDiskSizeInGb ]] ; then
    usage "--validator-additional-disk-size-gb currently only supported with cloud provider: gce"
  fi
  if [[ "$tmpfsAccounts" = "true" ]]; then
    usage "--tmpfs-accounts only supported on cloud provider: gce"
  fi
  if [[ "$customMemoryGB" != "$defaultCustomMemoryGB" ]]; then
    usage "--custom-memory-gb only supported on cloud provider: gce"
  fi
  ;;
*)
  echo "Error: Unknown cloud provider: $cloudProvider"
  ;;
esac

case $cloudProvider in
  gce | ec2 | azure)
    maybePreemptible="never preemptible"
    ;;
  colo)
    maybePreemptible=$preemptible
    ;;
  *)
    echo "Error: Unknown cloud provider: $cloudProvider"
    ;;
esac

if [[ $reclaimOnlyPreemptibleReservations == "true" && $reclaimAllReservations == "true" ]]; then
  usage "Cannot set both --reclaim-preemptible-reservations and --reclaim-all-reservations.  Set one or none"
fi

if [[ -n $reclaimAllReservations || -n $reclaimOnlyPreemptibleReservations ]]; then
  forceDelete="true"
fi

if [[ -n "$customMachineType" ]] ; then
  bootstrapLeaderMachineType="$customMachineType"
elif [[ "$enableGpu" = "true" ]] ; then
  bootstrapLeaderMachineType="$gpuBootstrapLeaderMachineType"
else
  bootstrapLeaderMachineType="$cpuBootstrapLeaderMachineType"
fi
validatorMachineType=$bootstrapLeaderMachineType
blockstreamerMachineType=$bootstrapLeaderMachineType

# cloud_ForEachInstance [cmd] [extra args to cmd]
#
# Execute a command for each element in the `instances` array
#
#   cmd   - The command to execute on each instance
#           The command will receive arguments followed by any
#           additional arguments supplied to cloud_ForEachInstance:
#               name     - name of the instance
#               publicIp - The public IP address of this instance
#               privateIp - The private IP address of this instance
#               zone     - Zone of this instance
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
    IFS=: read -r name publicIp privateIp zone < <(echo "$info")

    # shellcheck disable=SC2294
    eval "$cmd" "$name" "$publicIp" "$privateIp" "$zone" "$count" "$@"
    count=$((count + 1))
  done
}

# Given a cloud provider zone, return an approximate lat,long location for the
# data center.  Normal geoip lookups for cloud provider IP addresses are
# sometimes widely inaccurate.
zoneLocation() {
  declare zone="$1"
  case "$zone" in
  us-west1*)
    echo "[45.5946, -121.1787]"
    ;;
  us-central1*)
    echo "[41.2619, -95.8608]"
    ;;
  us-east1*)
    echo "[33.1960, -80.0131]"
    ;;
  asia-east2*)
    echo "[22.3193, 114.1694]"
    ;;
  asia-northeast1*)
    echo "[35.6762, 139.6503]"
    ;;
  asia-northeast2*)
    echo "[34.6937, 135.5023]"
    ;;
  asia-south1*)
    echo "[19.0760, 72.8777]"
    ;;
  asia-southeast1*)
    echo "[1.3404, 103.7090]"
    ;;
  australia-southeast1*)
    echo "[-33.8688, 151.2093]"
    ;;
  europe-north1*)
    echo "[60.5693, 27.1878]"
    ;;
  europe-west2*)
    echo "[51.5074, -0.1278]"
    ;;
  europe-west3*)
    echo "[50.1109, 8.6821]"
    ;;
  europe-west4*)
    echo "[53.4386, 6.8355]"
    ;;
  europe-west6*)
    echo "[47.3769, 8.5417]"
    ;;
  northamerica-northeast1*)
    echo "[45.5017, -73.5673]"
    ;;
  southamerica-east1*)
    echo "[-23.5505, -46.6333]"
    ;;
  *)
    ;;
  esac
}

prepareInstancesAndWriteConfigFile() {
  $metricsWriteDatapoint "testnet-deploy net-config-begin=1"

  if $externalNodes; then
    echo "Appending to existing config file"
    echo "externalNodeSshKey=$sshPrivateKey" >> "$configFile"
  else
    rm -f "$geoipConfigFile"
    cat >> "$configFile" <<EOF
# autogenerated at $(date)
netBasename=$prefix
publicNetwork=$publicNetwork
sshPrivateKey=$sshPrivateKey
letsEncryptDomainName=$letsEncryptDomainName
export TMPFS_ACCOUNTS=$tmpfsAccounts
EOF
  fi
  touch "$geoipConfigFile"

  buildSshOptions

  cloud_RestartPreemptedInstances "$prefix"

  fetchPrivateKey() {
    declare nodeName
    declare nodeIp
    declare nodeZone
    IFS=: read -r nodeName nodeIp _ nodeZone < <(echo "${instances[0]}")

    # Make sure the machine is alive or pingable
    timeout_sec=90
    cloud_WaitForInstanceReady "$nodeName" "$nodeIp" "$nodeZone" "$timeout_sec"

    if [[ ! -r $sshPrivateKey ]]; then
      echo "Fetching $sshPrivateKey from $nodeName"

      # Try to scp in a couple times, sshd may not yet be up even though the
      # machine can be pinged...
      (
        set -o pipefail
        for i in $(seq 1 60); do
          set -x
          cloud_FetchFile "$nodeName" "$nodeIp" /solana-scratch/id_ecdsa "$sshPrivateKey" "$nodeZone" &&
            cloud_FetchFile "$nodeName" "$nodeIp" /solana-scratch/id_ecdsa.pub "$sshPrivateKey.pub" "$nodeZone" &&
              break
          set +x

          sleep 1
          echo "Retry $i..."
        done
      )

      chmod 400 "$sshPrivateKey"
      ls -l "$sshPrivateKey"
    fi
  }

  recordInstanceIp() {
    declare name="$1"
    declare publicIp="$2"
    declare privateIp="$3"
    declare zone="$4"
    #declare index="$5"

    declare failOnFailure="$6"
    declare arrayName="$7"

    if [ "$publicIp" = "TERMINATED" ] || [ "$privateIp" = "TERMINATED" ]; then
      if $failOnFailure; then
        exit 1
      else
        return 0
      fi
    fi

    ok=true
    echo "Waiting for $name to finish booting..."
    (
      set +e
      fetchPrivateKey || exit 1
      for i in $(seq 1 60); do
        (
          set -x
          timeout --preserve-status --foreground 20s ssh "${sshOptions[@]}" "$publicIp" "ls -l /solana-scratch/.instance-startup-complete"
        )
        ret=$?
        if [[ $ret -eq 0 ]]; then
          echo "$name has booted."
          exit 0
        fi
        sleep 5
        echo "Retry $i..."
      done
      echo "$name failed to boot."
      exit 1
    ) || ok=false

    if ! $ok; then
      if $failOnFailure; then
        exit 1
      fi
    else
      {
        echo "$arrayName+=($publicIp)  # $name"
        echo "${arrayName}Private+=($privateIp)  # $name"
        echo "${arrayName}Zone+=($zone)  # $name"
      } >> "$configFile"

      declare latlng=
      latlng=$(zoneLocation "$zone")
      if [[ -n $latlng ]]; then
        echo "$publicIp: $latlng" >> "$geoipConfigFile"
      fi
    fi
  }

  if $externalNodes; then
    echo "Bootstrap validator is already configured"
  else
    echo "Looking for bootstrap validator instance..."
    cloud_FindInstance "$prefix-bootstrap-validator"
    [[ ${#instances[@]} -eq 1 ]] || {
      echo "Unable to find bootstrap validator"
      exit 1
    }

    echo "validatorIpList=()" >> "$configFile"
    echo "validatorIpListPrivate=()" >> "$configFile"
    cloud_ForEachInstance recordInstanceIp true validatorIpList
  fi

  if [[ $additionalValidatorCount -gt 0 ]]; then
    numZones=${#zones[@]}
    if [[ $additionalValidatorCount -gt $numZones ]]; then
      numNodesPerZone=$((additionalValidatorCount / numZones))
      numLeftOverNodes=$((additionalValidatorCount % numZones))
    else
      numNodesPerZone=1
      numLeftOverNodes=0
    fi

    for ((i=((numZones - 1)); i >= 0; i--)); do
      zone=${zones[i]}
      if [[ $i -eq 0 ]]; then
        numNodesPerZone=$((numNodesPerZone + numLeftOverNodes))
      fi
      echo "Looking for additional validator instances in $zone ..."
      cloud_FindInstances "$prefix-$zone-validator"
      declare numInstances=${#instances[@]}
      if [[ $numInstances -ge $numNodesPerZone || ( ! $failOnValidatorBootupFailure && $numInstances -gt 0 ) ]]; then
        cloud_ForEachInstance recordInstanceIp "$failOnValidatorBootupFailure" validatorIpList
      else
        echo "Unable to find additional validators"
        if $failOnValidatorBootupFailure; then
          exit 1
        fi
      fi
    done
  fi

  if ! $externalNodes; then
    echo "clientIpList=()" >> "$configFile"
    echo "clientIpListPrivate=()" >> "$configFile"
  fi
  echo "Looking for client bencher instances..."
  cloud_FindInstances "$prefix-client"
  [[ ${#instances[@]} -eq 0 ]] || {
    cloud_ForEachInstance recordInstanceIp true clientIpList
  }

  if ! $externalNodes; then
    echo "blockstreamerIpList=()" >> "$configFile"
    echo "blockstreamerIpListPrivate=()" >> "$configFile"
  fi
  echo "Looking for blockstreamer instances..."
  cloud_FindInstances "$prefix-blockstreamer"
  [[ ${#instances[@]} -eq 0 ]] || {
    cloud_ForEachInstance recordInstanceIp true blockstreamerIpList
  }

  echo "Wrote $configFile"
  $metricsWriteDatapoint "testnet-deploy net-config-complete=1"
}

delete() {
  $metricsWriteDatapoint "testnet-deploy net-delete-begin=1"

  case $cloudProvider in
    gce | ec2 | azure)
      # Filter for all nodes
      filter="$prefix-"
      ;;
    colo)
      if [[ -n $forceDelete ]]; then
        filter=".*-"
      else
        filter="$prefix-"
      fi
      ;;
    *)
      echo "Error: Unknown cloud provider: $cloudProvider"
      ;;
  esac

  echo "Searching for instances: $filter"
  cloud_FindInstances "$filter" "$reclaimOnlyPreemptibleReservations"

  if [[ ${#instances[@]} -eq 0 ]]; then
    echo "No instances found matching '$filter'"
  else
    cloud_DeleteInstances $forceDelete
  fi

  wait

  if $externalNodes; then
    echo "Let's not delete the current configuration file"
  else
    rm -f "$configFile"
  fi

  $metricsWriteDatapoint "testnet-deploy net-delete-complete=1"
}

create_error_cleanup() {
  declare RC=$?
  if [[ "$RC" -ne 0 ]]; then
    delete
  fi
  exit $RC
}

case $command in
delete)
  delete
  ;;

create)
  [[ -n $additionalValidatorCount ]] || usage "Need number of nodes"

  delete

  $metricsWriteDatapoint "testnet-deploy net-create-begin=1"

  if $failOnValidatorBootupFailure; then
    trap create_error_cleanup EXIT
  fi

  rm -rf "$sshPrivateKey"{,.pub}

  # Note: using rsa because |aws ec2 import-key-pair| seems to fail for ecdsa
  ssh-keygen -t rsa -N '' -f "$sshPrivateKey"

  printNetworkInfo() {
    cat <<EOF
==[ Network composition ]===============================================================
  Bootstrap validator = $bootstrapLeaderMachineType (GPU=$enableGpu)
  Additional validators = $additionalValidatorCount x $validatorMachineType
  Client(s) = $clientNodeCount x $clientMachineType
  Blockstreamer = $blockstreamer
========================================================================================

EOF
  }
  printNetworkInfo

  creationDate=$(date)
  creationInfo() {
    cat <<EOF

  Instance running since: $creationDate

========================================================================================
EOF
  }

  declare startupScript="$netConfigDir"/instance-startup-script.sh
  cat > "$startupScript" <<EOF
#!/usr/bin/env bash
# autogenerated at $(date)
set -ex

if [[ -f /solana-scratch/.instance-startup-complete ]]; then
  echo reboot
  $(
    cd "$here"/scripts/
    if "$enableGpu"; then
      cat enable-nvidia-persistence-mode.sh
    fi

    if [[ -n $validatorAdditionalDiskSizeInGb ]]; then
      cat mount-additional-disk.sh
    fi

    cat ../../scripts/ulimit-n.sh
  )
  if [[ -x ~solana/solana/on-reboot ]]; then
    sudo -u solana ~solana/solana/on-reboot
  fi

  # Skip most setup on instance reboot
  exit 0
fi

cat > /etc/motd <<EOM
!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!

  This instance has not been fully configured.

  See startup script log messages in /var/log/syslog for status:
    $ sudo cat /var/log/syslog | egrep \\(startup-script\\|cloud-init\)

  To block until setup is complete, run:
    $ until [[ -f /solana-scratch/.instance-startup-complete ]]; do sleep 1; done

!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!
$(creationInfo)
EOM

# Place the generated private key at /solana-scratch/id_ecdsa so it's retrievable by anybody
# who is able to log into this machine
mkdir -m 0777 /solana-scratch
cat > /solana-scratch/id_ecdsa <<EOK
$(cat "$sshPrivateKey")
EOK
cat > /solana-scratch/id_ecdsa.pub <<EOK
$(cat "$sshPrivateKey.pub")
EOK
chmod 444 /solana-scratch/id_ecdsa

USER=\$(id -un)
export DEBIAN_FRONTEND=noninteractive
$(
  cd "$here"/scripts/
  cat \
    disable-background-upgrades.sh \
    create-solana-user.sh \
    solana-user-authorized_keys.sh \
    add-testnet-solana-user-authorized_keys.sh \
    install-ag.sh \
    install-certbot.sh \
    install-earlyoom.sh \
    install-iftop.sh \
    install-libssl-compatability.sh \
    install-redis.sh \
    install-rsync.sh \
    install-perf.sh \
    localtime.sh \
    network-config.sh \
    remove-docker-interface.sh \

  if "$enableGpu"; then
    cat enable-nvidia-persistence-mode.sh
  fi

  if [[ -n $validatorAdditionalDiskSizeInGb ]]; then
    cat mount-additional-disk.sh
  fi

  if [[ $selfDestructHours -gt 0 ]]; then
    cat <<EOSD

# Setup GCE self-destruct
cat >/solana-scratch/gce-self-destruct.sh <<'EOS'
$(cat gce-self-destruct.sh)
EOS
EOSD
    cat <<'EOSD'

# Populate terminal prompt update script
cat >/solana-scratch/gce-self-destruct-ps1.sh <<'EOS'
#!/usr/bin/env bash
source "$(dirname "$0")/gce-self-destruct.sh"
gce_self_destruct_ps1
EOS
chmod +x /solana-scratch/gce-self-destruct-ps1.sh

# Append MOTD and PS1 replacement to .profile
cat >>~solana/.profile <<'EOS'

# Print self-destruct countdown on login
source "/solana-scratch/gce-self-destruct.sh"
gce_self_destruct_motd

# Add self-destruct countdown to terminal prompt
export PS1='\[\e]0;\u@\h: \w\a\]${debian_chroot:+($debian_chroot)}\[\033[01;32m\]\u@\h\[\033[00m\]$(/solana-scratch/gce-self-destruct-ps1.sh):\[\033[01;34m\]\w\[\033[00m\]\$ '
EOS
EOSD
    cat <<EOSD

source /solana-scratch/gce-self-destruct.sh
gce_self_destruct_setup $selfDestructHours
EOSD
  fi
)

cat > /etc/motd <<EOM
See startup script log messages in /var/log/syslog for status:
  $ sudo cat /var/log/syslog | egrep \\(startup-script\\|cloud-init\)
$(printNetworkInfo)
$(creationInfo)
EOM

$(
  if [[ "$tmpfsAccounts" = "true" ]]; then
    cat <<'EOSWAP'

# Setup swap/tmpfs for accounts
tmpfsMountPoint=/mnt/solana-accounts
swapDevice="/dev/nvme0n1"
swapUUID="43076c54-7840-4e59-a368-2d164f8984fb"
mkswap --uuid "$swapUUID" "$swapDevice"
echo "UUID=$swapUUID swap swap defaults 0 0" >> /etc/fstab
swapon "UUID=$swapUUID"
mkdir -p -m 0777 "$tmpfsMountPoint"
echo "tmpfs $tmpfsMountPoint tmpfs defaults,size=300G 0 0" >> /etc/fstab
mount "$tmpfsMountPoint"
EOSWAP
  fi
)

touch /solana-scratch/.instance-startup-complete

EOF

  if $blockstreamer; then
    blockstreamerAddress=$customAddress
  else
    bootstrapLeaderAddress=$customAddress
  fi

  for zone in "${zones[@]}"; do
    cloud_Initialize "$prefix" "$zone"
  done

  if $externalNodes; then
    echo "Bootstrap validator is already configured"
  else
    cloud_CreateInstances "$prefix" "$prefix-bootstrap-validator" 1 \
      "$enableGpu" "$bootstrapLeaderMachineType" "${zones[0]}" "$validatorBootDiskSizeInGb" \
      "$startupScript" "$bootstrapLeaderAddress" "$bootDiskType" "$validatorAdditionalDiskSizeInGb" \
      "$maybePreemptible" "$sshPrivateKey"
  fi

  if [[ $additionalValidatorCount -gt 0 ]]; then
    num_zones=${#zones[@]}
    if [[ $additionalValidatorCount -gt $num_zones ]]; then
      numNodesPerZone=$((additionalValidatorCount / num_zones))
      numLeftOverNodes=$((additionalValidatorCount % num_zones))
    else
      numNodesPerZone=1
      numLeftOverNodes=0
    fi

    for ((i=((num_zones - 1)); i >= 0; i--)); do
      zone=${zones[i]}
      if [[ $i -eq 0 ]]; then
        numNodesPerZone=$((numNodesPerZone + numLeftOverNodes))
      fi
      cloud_CreateInstances "$prefix" "$prefix-$zone-validator" "$numNodesPerZone" \
        "$enableGpu" "$validatorMachineType" "$zone" "$validatorBootDiskSizeInGb" \
        "$startupScript" "" "$bootDiskType" "$validatorAdditionalDiskSizeInGb" \
        "$preemptible" "$sshPrivateKey" &
    done

    wait
  fi

  if [[ $clientNodeCount -gt 0 ]]; then
    cloud_CreateInstances "$prefix" "$prefix-client" "$clientNodeCount" \
      "$enableGpu" "$clientMachineType" "${zones[0]}" "$clientBootDiskSizeInGb" \
      "$startupScript" "" "$bootDiskType" "" "$maybePreemptible" "$sshPrivateKey"
  fi

  if $blockstreamer; then
    cloud_CreateInstances "$prefix" "$prefix-blockstreamer" "1" \
      "$enableGpu" "$blockstreamerMachineType" "${zones[0]}" "$validatorBootDiskSizeInGb" \
      "$startupScript" "$blockstreamerAddress" "$bootDiskType" "" "$maybePreemptible" "$sshPrivateKey"
  fi

  $metricsWriteDatapoint "testnet-deploy net-create-complete=1"

  prepareInstancesAndWriteConfigFile
  ;;

config)
  failOnValidatorBootupFailure=false
  prepareInstancesAndWriteConfigFile
  ;;
info)
  loadConfigFile
  printNode() {
    declare nodeType=$1
    declare ip=$2
    declare ipPrivate=$3
    declare zone=$4
    printf "  %-16s | %-15s | %-15s | %s\n" "$nodeType" "$ip" "$ipPrivate" "$zone"
  }

  if $evalInfo; then
    echo "NET_NUM_VALIDATORS=${#validatorIpList[@]}"
    echo "NET_NUM_CLIENTS=${#clientIpList[@]}"
    echo "NET_NUM_BLOCKSTREAMERS=${#blockstreamerIpList[@]}"
  else
    printNode "Node Type" "Public IP" "Private IP" "Zone"
    echo "-------------------+-----------------+-----------------+--------------"
  fi

  nodeType=bootstrap-validator
  if [[ ${#validatorIpList[@]} -gt 0 ]]; then
    for i in $(seq 0 $(( ${#validatorIpList[@]} - 1)) ); do
      ipAddress=${validatorIpList[$i]}
      ipAddressPrivate=${validatorIpListPrivate[$i]}
      zone=${validatorIpListZone[$i]}
      if $evalInfo; then
        echo "NET_VALIDATOR${i}_IP=$ipAddress"
      else
        printNode $nodeType "$ipAddress" "$ipAddressPrivate" "$zone"
      fi
      nodeType=validator
    done
  fi

  if [[ ${#clientIpList[@]} -gt 0 ]]; then
    for i in $(seq 0 $(( ${#clientIpList[@]} - 1)) ); do
      ipAddress=${clientIpList[$i]}
      ipAddressPrivate=${clientIpListPrivate[$i]}
      zone=${clientIpListZone[$i]}
      if $evalInfo; then
        echo "NET_CLIENT${i}_IP=$ipAddress"
      else
        printNode client "$ipAddress" "$ipAddressPrivate" "$zone"
      fi
    done
  fi

  if [[ ${#blockstreamerIpList[@]} -gt 0 ]]; then
    for i in $(seq 0 $(( ${#blockstreamerIpList[@]} - 1)) ); do
      ipAddress=${blockstreamerIpList[$i]}
      ipAddressPrivate=${blockstreamerIpListPrivate[$i]}
      zone=${blockstreamerIpListZone[$i]}
      if $evalInfo; then
        echo "NET_BLOCKSTREAMER${i}_IP=$ipAddress"
      else
        printNode blockstreamer "$ipAddress" "$ipAddressPrivate" "$zone"
      fi
    done
  fi

  ;;
status)
  cloud_StatusAll
  ;;
*)
  usage "Unknown command: $command"
esac
