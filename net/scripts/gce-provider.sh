# |source| this file
#
# Utilities for working with GCE instances
#

# Default zone
cloud_DefaultZone() {
  echo "us-west1-b"
}

#
# cloud_RestartPreemptedInstances [namePrefix]
#
# Restart any preempted instances matching the specified prefix
#
# namePrefix - The instance name prefix of the preempted instances
#
cloud_RestartPreemptedInstances() {
  declare filter="$1"

  declare name status zone
  while read -r name status zone; do
    echo "Starting $status instance: $name"
    (
      set -x
      gcloud compute instances start --zone "$zone" "$name"
    )
  done < <(gcloud compute instances list \
             --filter "$filter" \
             --format 'value(name,status,zone)' \
           | grep TERMINATED)
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
  while read -r name publicIp privateIp status zone; do
    printf "%-30s | publicIp=%-16s privateIp=%s status=%s zone=%s\n" "$name" "$publicIp" "$privateIp" "$status" "$zone"

    instances+=("$name:$publicIp:$privateIp:$zone")
  done < <(gcloud compute instances list \
             --filter "$filter" \
             --format 'value(name,networkInterfaces[0].accessConfigs[0].natIP,networkInterfaces[0].networkIP,status,zone)' \
           | grep RUNNING)

  while read -r name status zone; do
    privateIp=TERMINATED
    publicIp=TERMINATED
    printf "%-30s | publicIp=%-16s privateIp=%s status=%s zone=%s\n" "$name" "$publicIp" "$privateIp" "$status" "$zone"

    instances+=("$name:$publicIp:$privateIp:$zone")
  done < <(gcloud compute instances list \
             --filter "$filter" \
             --format 'value(name,status,zone)' \
           | grep TERMINATED)
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
# cloud_Initialize [networkName]
#
# Perform one-time initialization that may be required for the given testnet.
#
# networkName   - unique name of this testnet
#
# This function will be called before |cloud_CreateInstances|
cloud_Initialize() {
  declare networkName="$1"
  # ec2-provider.sh creates firewall rules programmatically, should do the same
  # here.
  echo "Note: one day create $networkName firewall rules programmatically instead of assuming the 'testnet' tag exists"
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
# bootDiskType  - Optional specify SSD or HDD boot disk
# additionalDiskSize - Optional specify size of additional storage volume
# preemptible - Optionally request a preemptible instance ("true")
#
# Tip: use cloud_FindInstances to locate the instances once this function
#      returns
cloud_CreateInstances() {
  declare networkName="$1"
  declare namePrefix="$2"
  declare numNodes="$3"
  declare enableGpu="$4"
  declare machineType="$5"
  declare zone="$6"
  declare optionalBootDiskSize="$7"
  declare optionalStartupScript="$8"
  declare optionalAddress="$9"
  declare optionalBootDiskType="${10}"
  declare optionalAdditionalDiskSize="${11}"
  declare optionalPreemptible="${12}"
  #declare sshPrivateKey="${13}"  # unused

  if $enableGpu; then
    # Custom Ubuntu 18.04 LTS image with CUDA 9.2 and CUDA 10.0 installed
    #
    # Unfortunately this image is not public.  When this becomes an issue, use
    # the stock Ubuntu 18.04 image and programmatically install CUDA after the
    # instance boots
    #
    imageName="ubuntu-1804-bionic-v20181029-with-cuda-10-and-cuda-9-2"
  else
    # Upstream Ubuntu 18.04 LTS image
    imageName="ubuntu-1804-bionic-v20190813a --image-project ubuntu-os-cloud"
  fi

  declare -a nodes
  if [[ $numNodes = 1 ]]; then
    nodes=("$namePrefix")
  else
    for node in $(seq -f "${namePrefix}%0${#numNodes}g" 1 "$numNodes"); do
      nodes+=("$node")
    done
  fi

  declare -a args
  args=(
    --zone "$zone"
    --tags testnet
    --metadata "testnet=$networkName"
    --image "$imageName"
    --maintenance-policy TERMINATE
    --restart-on-failure
  )

  # shellcheck disable=SC2206 # Do not want to quote $imageName as it may contain extra args
  args+=(--image $imageName)

  if [[ $optionalPreemptible = true ]]; then
    args+=(--preemptible)
  fi

  # shellcheck disable=SC2206 # Do not want to quote $machineType as it may contain extra args
  for word in $machineType; do
    # Special handling for the "--min-cpu-platform" argument which may contain a
    # space (escaped as '%20')...
    args+=("${word//%20/ }")
  done
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

  if [[ -n $optionalAdditionalDiskSize ]]; then
    if [[ $numNodes = 1 ]]; then
      (
        set -x
        cloud_CreateAndAttachPersistentDisk "${namePrefix}" "$optionalAdditionalDiskSize" "pd-ssd" "$zone"
      )
    else
      for node in $(seq -f "${namePrefix}%0${#numNodes}g" 1 "$numNodes"); do
        (
          set -x
          cloud_CreateAndAttachPersistentDisk "${node}" "$optionalAdditionalDiskSize" "pd-ssd" "$zone"
        )
      done
    fi
  fi
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
  declare zones=("${instances[@]/*:/}")
  declare unique_zones=()
  read -r -a unique_zones <<< "$(echo "${zones[@]}" | tr ' ' '\n' | sort -u | tr '\n' ' ')"

  for zone in "${unique_zones[@]}"; do
    set -x
    # Try deleting instances in all zones
    gcloud beta compute instances delete --zone "$zone" --quiet "${names[@]}" || true
  done
}

#
# cloud_WaitForInstanceReady [instanceName] [instanceIp] [instanceZone] [timeout]
#
# Return once the newly created VM instance is responding.  This function is cloud-provider specific.
#
cloud_WaitForInstanceReady() {
  declare instanceName="$1"
  declare instanceIp="$2"
#  declare instanceZone="$3"
  declare timeout="$4"

  if [[ $instanceIp = "TERMINATED" ]]; then
    return 1
  fi
  timeout "${timeout}"s bash -c "set -o pipefail; until ping -c 3 $instanceIp | tr - _; do echo .; done"
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
  declare zone="$5"

  if [[ $publicIp = "TERMINATED" ]]; then
    return 1
  fi

  (
    set -x
    gcloud compute scp --zone "$zone" "$instanceName:$remoteFile" "$localFile"
  )
}

#
# cloud_CreateAndAttachPersistentDisk [instanceName] [diskSize] [diskType]
#
# Create a persistent disk and attach it to a pre-existing VM instance.
# Set disk to auto-delete upon instance deletion
#
cloud_CreateAndAttachPersistentDisk() {
  declare instanceName="$1"
  declare diskSize="$2"
  declare diskType="$3"
  declare zone="$4"
  diskName="${instanceName}-pd"

  gcloud beta compute disks create "$diskName" \
    --size "$diskSize" \
    --type "$diskType" \
    --zone "$zone"

  gcloud compute instances attach-disk "$instanceName" \
    --disk "$diskName" \
    --zone "$zone"

  gcloud compute instances set-disk-auto-delete "$instanceName" \
    --disk "$diskName" \
    --zone "$zone" \
    --auto-delete
}

#
# cloud_StatusAll
#
# Not yet implemented for this cloud provider
cloud_StatusAll() {
  echo "ERROR: cloud_StatusAll is not yet implemented for GCE"
}
