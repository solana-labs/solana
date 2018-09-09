#!/bin/bash -e

here=$(dirname "$0")
# shellcheck source=net/scripts/gcloud.sh
source "$here"/scripts/gcloud.sh
# shellcheck source=net/common.sh
source "$here"/common.sh

prefix=testnet-dev-${USER//[^A-Za-z0-9]/}
validatorNodeCount=5
clientNodeCount=1
leaderBootDiskSize=1TB
leaderMachineType=n1-standard-16
leaderAccelerator=
validatorMachineType=n1-standard-4
validatorBootDiskSize=$leaderBootDiskSize
validatorAccelerator=
clientMachineType=n1-standard-16
clientBootDiskSize=40GB
clientAccelerator=

imageName="ubuntu-16-04-cuda-9-2-new"
publicNetwork=false
zone="us-west1-b"
leaderAddress=

usage() {
  exitcode=0
  if [[ -n "$1" ]]; then
    exitcode=1
    echo "Error: $*"
  fi
  cat <<EOF
usage: $0 [create|config|delete] [common options] [command-specific options]

Configure a GCE-based testnet

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
   -z [zone]        - GCP Zone for the nodes (default: $zone)
   -i [imageName]   - Existing image on GCE (default: $imageName)
   -g               - Enable GPU
   -a [address]     - Set the leader node's external IP address to this GCE address

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

while getopts "h?p:Pi:n:c:z:ga:" opt; do
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
  i)
    imageName=$OPTARG
    ;;
  n)
    validatorNodeCount=$OPTARG
    ;;
  c)
    clientNodeCount=$OPTARG
    ;;
  z)
    zone=$OPTARG
    ;;
  g)
    leaderAccelerator="count=4,type=nvidia-tesla-k80"
    ;;
  a)
    leaderAddress=$OPTARG
    ;;
  *)
    usage "Error: unhandled option: $opt"
    ;;
  esac
done
shift $((OPTIND - 1))

[[ -z $1 ]] || usage "Unexpected argument: $1"
sshPrivateKey="$netConfigDir/id_$prefix"

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
    declare publicIp="$3"
    declare privateIp="$4"

    declare arrayName="$6"

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
    declare publicIp="$3"

    echo "Waiting for $name to finish booting..."
    (
      for i in $(seq 1 30); do
        if (set -x; ssh "${sshOptions[@]}" "$publicIp" "test -f /.gce-startup-complete"); then
          break
        fi
        sleep 2
        echo "Retry $i..."
      done
    )
  }

  echo "Looking for leader instance..."
  gcloud_FindInstances "name=$prefix-leader" show
  [[ ${#instances[@]} -eq 1 ]] || {
    echo "Unable to find leader"
    exit 1
  }

  echo "Fetching $sshPrivateKey from $leaderName"
  (
    rm -rf "$sshPrivateKey"{,pub}

    declare leaderName
    declare leaderZone
    declare leaderIp
    IFS=: read -r leaderName leaderZone leaderIp _ < <(echo "${instances[0]}")

    set -x

    # Try to ping the machine first.  There can be a delay between when the
    # instance is reported as RUNNING and when it's reachable over the network
    timeout 30s bash -c "set -o pipefail; until ping -c 3 $leaderIp | tr - _; do echo .; done"

    # Try to scp in a couple times, sshd may not yet be up even though the
    # machine can be pinged...
    set -o pipefail
    for i in $(seq 1 10); do
      if gcloud compute scp --zone "$leaderZone" \
          "$leaderName:/solana-id_ecdsa" "$sshPrivateKey"; then
        break
      fi
      sleep 1
      echo "Retry $i..."
    done

    chmod 400 "$sshPrivateKey"
  )

  echo "leaderIp=()" >> "$configFile"
  gcloud_ForEachInstance recordInstanceIp leaderIp
  gcloud_ForEachInstance waitForStartupComplete

  echo "Looking for validator instances..."
  gcloud_FindInstances "name~^$prefix-validator" show
  [[ ${#instances[@]} -gt 0 ]] || {
    echo "Unable to find validators"
    exit 1
  }
  echo "validatorIpList=()" >> "$configFile"
  gcloud_ForEachInstance recordInstanceIp validatorIpList
  gcloud_ForEachInstance waitForStartupComplete

  echo "clientIpList=()" >> "$configFile"
  echo "Looking for client instances..."
  gcloud_FindInstances "name~^$prefix-client" show
  [[ ${#instances[@]} -eq 0 ]] || {
    gcloud_ForEachInstance recordInstanceIp clientIpList
    gcloud_ForEachInstance waitForStartupComplete
  }

  echo "Wrote $configFile"
  $metricsWriteDatapoint "testnet-deploy net-config-complete=1"
}

case $command in
delete)
  $metricsWriteDatapoint "testnet-deploy net-delete-begin=1"

  # Delete the leader node first to prevent unusual metrics on the dashboard
  # during shutdown.
  # TODO: It would be better to fully cut-off metrics reporting before any
  # instances are deleted.
  for filter in "^$prefix-leader" "^$prefix-"; do
    gcloud_FindInstances "name~$filter"

    if [[ ${#instances[@]} -eq 0 ]]; then
      echo "No instances found matching '$filter'"
    else
      gcloud_DeleteInstances true
    fi
  done
  rm -f "$configFile"

  $metricsWriteDatapoint "testnet-deploy net-delete-complete=1"
  ;;

create)
  [[ -n $validatorNodeCount ]] || usage "Need number of nodes"

  $metricsWriteDatapoint "testnet-deploy net-create-begin=1"

  rm -rf "$sshPrivateKey"{,.pub}
  ssh-keygen -t ecdsa -N '' -f "$sshPrivateKey"

  printNetworkInfo() {
    cat <<EOF
========================================================================================

Network composition:
  Leader = $leaderMachineType (GPU=${leaderAccelerator:-none})
  Validators = $validatorNodeCount x $validatorMachineType (GPU=${validatorAccelerator:-none})
  Client(s) = $clientNodeCount x $clientMachineType (GPU=${clientAccelerator:-none})

========================================================================================

EOF
  }
  printNetworkInfo

  declare startupScript="$netConfigDir"/gce-startup-script.sh
  cat > "$startupScript" <<EOF
#!/bin/bash -ex
# autogenerated at $(date)

cat > /etc/motd <<EOM
!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!

  This instance has not been fully configured.
  See "startup-script" log messages in /var/log/syslog for status:
    $ sudo cat /var/log/syslog | grep startup-script

  To block until setup is complete, run:
    $ until [[ -f /.gce-startup-complete ]]; do sleep 1; done

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
    install-earlyoom.sh \
    install-rsync.sh \
    install-libssl-compatability.sh \
)

cat > /etc/motd <<EOM
$(printNetworkInfo)
EOM

touch /.gce-startup-complete

EOF

  gcloud_CreateInstances "$prefix-leader" 1 "$zone" \
    "$imageName" "$leaderMachineType" "$leaderBootDiskSize" "$leaderAccelerator" \
    "$startupScript" "$leaderAddress"

  gcloud_CreateInstances "$prefix-validator" "$validatorNodeCount" "$zone" \
    "$imageName" "$validatorMachineType" "$validatorBootDiskSize" "$validatorAccelerator" \
    "$startupScript" ""

  if [[ $clientNodeCount -gt 0 ]]; then
    gcloud_CreateInstances "$prefix-client" "$clientNodeCount" "$zone" \
      "$imageName" "$clientMachineType" "$clientBootDiskSize" "$clientAccelerator" \
      "$startupScript" ""
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
