# |source| this file
#
# Utilities for working with GCE instances
#

# Default zone
zone="us-west1-b"
cloud_SetZone() {
  zone="$1"
}


#
# __cloud_FindInstances
#
# Find instances matching the specified pattern.
#
# For each matching instance, an entry in the `instances` array will be added with the
# following information about the instance:
#   "name:zone:public IP:private IP"
#
# filter   - The instances to filter on
#
# examples:
#   $ __cloud_FindInstances "name=exact-machine-name"
#   $ __cloud_FindInstances "name~^all-machines-with-a-common-machine-prefix"
#
__cloud_FindInstances() {
  declare filter="$1"
  instances=()

  declare name zone publicIp privateIp status
  while read -r name publicIp privateIp status; do
    printf "%-30s | publicIp=%-16s privateIp=%s status=%s\n" "$name" "$publicIp" "$privateIp" "$status"

    instances+=("$name:$publicIp:$privateIp")
  done < <(gcloud compute instances list \
             --filter "$filter" \
             --format 'value(name,networkInterfaces[0].accessConfigs[0].natIP,networkInterfaces[0].networkIP,status)')
}
#
# cloud_FindInstances [namePrefix]
#
# Find instances with names matching the specified prefix
#
# For each matching instance, an entry in the `instances` array will be added with the
# following information about the instance:
#   "name:public IP:private IP"
#
# namePrefix - The instance name prefix to look for
#
# examples:
#   $ cloud_FindInstances all-machines-with-a-common-machine-prefix
#
cloud_FindInstances() {
  declare namePrefix="$1"
  __cloud_FindInstances "name~^$namePrefix"
}

#
# cloud_FindInstance [name]
#
# Find an instance with a name matching the exact pattern.
#
# For each matching instance, an entry in the `instances` array will be added with the
# following information about the instance:
#   "name:public IP:private IP"
#
# name - The instance name to look for
#
# examples:
#   $ cloud_FindInstance exact-machine-name
#
cloud_FindInstance() {
  declare name="$1"
  __cloud_FindInstances "name=$name"
}

#
# cloud_CreateInstances [networkName] [namePrefix] [numNodes] [imageName]
#                       [machineType] [bootDiskSize] [enableGpu]
#                       [startupScript] [address]
#
# Creates one more identical instances.
#
# networkName   - unique name of this testnet
# namePrefix    - unique string to prefix all the instance names with
# numNodes      - number of instances to create
# imageName     - Disk image for the instances
# machineType   - GCE machine type.  Note that this may also include an
#                 `--accelerator=` or other |gcloud compute instances create|
#                 options
# bootDiskSize  - Optional size of the boot disk in GB
# enableGpu     - Optionally enable GPU, use the value "true" to enable
#                 eg, request 4 K80 GPUs with "count=4,type=nvidia-tesla-k80"
# startupScript - Optional startup script to execute when the instance boots
# address       - Optional name of the GCE static IP address to attach to the
#                 instance.  Requires that |numNodes| = 1 and that addressName
#                 has been provisioned in the GCE region that is hosting `$zone`
#
# Tip: use cloud_FindInstances to locate the instances once this function
#      returns
cloud_CreateInstances() {
  declare networkName="$1"
  declare namePrefix="$2"
  declare numNodes="$3"
  declare imageName="$4"
  declare machineType="$5"
  declare optionalBootDiskSize="$6"
  declare optionalStartupScript="$7"
  declare optionalAddress="$8"
  declare optionalBootDiskType="$9"

  declare nodes
  if [[ $numNodes = 1 ]]; then
    nodes=("$namePrefix")
  else
    read -ra nodes <<<$(seq -f "${namePrefix}%0${#numNodes}g" 1 "$numNodes")
  fi

  declare -a args
  args=(
    --zone "$zone"
    --tags testnet
    --metadata "testnet=$networkName"
    --image "$imageName"
    --maintenance-policy TERMINATE
    --no-restart-on-failure
  )

  # shellcheck disable=SC2206 # Do not want to quote $imageName as it may contain extra args
  args+=(--image $imageName)

  # shellcheck disable=SC2206 # Do not want to quote $machineType as it may contain extra args
  args+=(--machine-type $machineType)
  if [[ -n $optionalBootDiskSize ]]; then
    args+=(
      --boot-disk-size "${optionalBootDiskSize}GB"
    )
  fi
  if [[ -n $optionalStartupScript ]]; then
    args+=(
      --metadata-from-file "startup-script=$optionalStartupScript"
    )
  fi
  if [[ -n $optionalBootDiskType ]]; then
    args+=(
        --boot-disk-type "${optionalBootDiskType}"
    )
  fi

  if [[ -n $optionalAddress ]]; then
    [[ $numNodes = 1 ]] || {
      echo "Error: address may not be supplied when provisioning multiple nodes: $optionalAddress"
      exit 1
    }
    args+=(
      --address "$optionalAddress"
    )
  fi

  (
    set -x
    gcloud beta compute instances create "${nodes[@]}" "${args[@]}"
  )
}

#
# cloud_DeleteInstances
#
# Deletes all the instances listed in the `instances` array
#
cloud_DeleteInstances() {
  if [[ ${#instances[0]} -eq 0 ]]; then
    echo No instances to delete
    return
  fi
  declare names=("${instances[@]/:*/}")

  (
    set -x
    gcloud beta compute instances delete --zone "$zone" --quiet "${names[@]}"
  )
}


#
# cloud_FetchFile [instanceName] [publicIp] [remoteFile] [localFile]
#
# Fetch a file from the given instance.  This function uses a cloud-specific
# mechanism to fetch the file
#
cloud_FetchFile() {
  declare instanceName="$1"
  # shellcheck disable=SC2034 # publicIp is unused
  declare publicIp="$2"
  declare remoteFile="$3"
  declare localFile="$4"

  (
    set -x
    gcloud compute scp --zone "$zone" "$instanceName:$remoteFile" "$localFile"
  )
}
