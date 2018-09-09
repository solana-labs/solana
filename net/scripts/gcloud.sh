# |source| this file
#
# Utilities for working with gcloud
#


#
# gcloud_FindInstances [filter] [options]
#
# Find instances matching the specified pattern.
#
# For each matching instance, an entry in the `instances` array will be added with the
# following information about the instance:
#   "name:zone:public IP:private IP"
#
# filter   - The instances to filter on
# options  - If set to the string "show", the list of instances will be echoed
#            to stdout
#
# examples:
#   $ gcloud_FindInstances "name=exact-machine-name"
#   $ gcloud_FindInstances "name~^all-machines-with-a-common-machine-prefix"
#
gcloud_FindInstances() {
  declare filter="$1"
  declare options="$2"
  instances=()

  declare name zone publicIp privateIp status
  while read -r name zone publicIp privateIp status; do
    if [[ $status != RUNNING ]]; then
      echo "Warning: $name is not RUNNING, ignoring it."
      continue
    fi
    if [[ $options = show ]]; then
      printf "%-30s | %-16s publicIp=%-16s privateIp=%s\n" "$name" "$zone" "$publicIp" "$privateIp"
    fi

    instances+=("$name:$zone:$publicIp:$privateIp")
  done < <(gcloud compute instances list \
             --filter="$filter" \
             --format 'value(name,zone,networkInterfaces[0].accessConfigs[0].natIP,networkInterfaces[0].networkIP,status)')
}

#
# gcloud_ForEachInstance [cmd] [extra args to cmd]
#
# Execute a command for each element in the `instances` array
#
#   cmd   - The command to execute on each instance
#           The command will receive arguments followed by any
#           additionl arguments supplied to gcloud_ForEachInstance:
#               name     - name of the instance
#               zone     - zone the instance is located in
#               publicIp - The public IP address of this instance
#               privateIp - The priate IP address of this instance
#               count    - Monotonically increasing count for each
#                          invocation of cmd, starting at 1
#               ...      - Extra args to cmd..
#
#
gcloud_ForEachInstance() {
  declare cmd="$1"
  shift
  [[ -n $cmd ]] || { echo gcloud_ForEachInstance: cmd not specified; exit 1; }

  declare count=1
  for info in "${instances[@]}"; do
    declare name zone publicIp privateIp
    IFS=: read -r name zone publicIp privateIp < <(echo "$info")

    eval "$cmd" "$name" "$zone" "$publicIp" "$privateIp" "$count" "$@"
    count=$((count + 1))
  done
}

#
# gcloud_CreateInstances [namePrefix] [numNodes] [zone] [imageName]
#                        [machineType] [bootDiskSize] [accelerator]
#                        [startupScript] [address]
#
# Creates one more identical instances.
#
# namePrefix    - unique string to prefix all the instance names with
# numNodes      - number of instances to create
# zone          - zone to create the instances in
# imageName     - Disk image for the instances
# machineType   - GCE machine type
# bootDiskSize  - Optional disk of the boot disk
# accelerator   - Optional accelerator to attach to the instance(s), see
#                 eg, request 4 K80 GPUs with "count=4,type=nvidia-tesla-k80"
# startupScript - Optional startup script to execute when the instance boots
# address       - Optional name of the GCE static IP address to attach to the
#                 instance.  Requires that |numNodes| = 1 and that addressName
#                 has been provisioned in the GCE region that is hosting |zone|
#
# Tip: use gcloud_FindInstances to locate the instances once this function
#      returns
gcloud_CreateInstances() {
  declare namePrefix="$1"
  declare numNodes="$2"
  declare zone="$3"
  declare imageName="$4"
  declare machineType="$5"
  declare optionalBootDiskSize="$6"
  declare optionalAccelerator="$7"
  declare optionalStartupScript="$8"
  declare optionalAddress="$9"

  declare nodes
  if [[ $numNodes = 1 ]]; then
    nodes=("$namePrefix")
  else
    read -ra nodes <<<$(seq -f "${namePrefix}%0${#numNodes}g" 1 "$numNodes")
  fi

  declare -a args
  args=(
    "--zone=$zone"
    "--tags=testnet"
    "--image=$imageName"
    "--machine-type=$machineType"
  )
  if [[ -n $optionalBootDiskSize ]]; then
    args+=(
      "--boot-disk-size=$optionalBootDiskSize"
    )
  fi
  if [[ -n $optionalAccelerator ]]; then
    args+=(
      "--accelerator=$optionalAccelerator"
      --maintenance-policy TERMINATE
      --restart-on-failure
    )
  fi
  if [[ -n $optionalStartupScript ]]; then
    args+=(
      --metadata-from-file "startup-script=$optionalStartupScript"
    )
  fi

  if [[ -n $optionalAddress ]]; then
    [[ $numNodes = 1 ]] || {
      echo "Error: address may not be supplied when provisioning multiple nodes: $optionalAddress"
      exit 1
    }
    args+=(
      "--address=$optionalAddress"
    )
  fi

  (
    set -x
    gcloud beta compute instances create "${nodes[@]}" "${args[@]}"
  )
}

#
# gcloud_DeleteInstances [yes]
#
# Deletes all the instances listed in the `instances` array
#
# If yes = "true", skip the delete confirmation
#
gcloud_DeleteInstances() {
  declare maybeQuiet=
  if [[ $1 = true ]]; then
    maybeQuiet=--quiet
  fi

  if [[ ${#instances[0]} -eq 0 ]]; then
    echo No instances to delete
    return
  fi
  declare names=("${instances[@]/:*/}")

  # Assume all instances are in the same zone
  # TODO: One day this assumption will be invalid
  declare zone
  IFS=: read -r _ zone _ < <(echo "${instances[0]}")

  (
    set -x
    gcloud beta compute instances delete --zone "$zone" $maybeQuiet "${names[@]}"
  )
}

