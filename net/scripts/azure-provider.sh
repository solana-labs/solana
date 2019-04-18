# |source| this file
#
# Utilities for working with Azure instances
#
set -x
# Default zone
cloud_DefaultZone() {
  echo "westus"
}

#
# __cloud_GetConfigValueFromInstanceName
# Return a piece of configuration information about an instance
# Provide the exact name of an instance and the configuration key, and the corresponding value will be returned
#
# example:
#   This will return the name of the resource group of the instance named
#   __cloud_GetConfigValueFromInstanceName some-instance-name resourceGroup

cloud_GetConfigValueFromInstanceName() {
  query="[?name=='$1']"
  key="[$2]"
  config_value=`az vm list -d -o tsv --query "$query.$key"`
}

cloud_GetResourceGroupFromInstance() {
  resourceGroup=`az vm list -o tsv --query "[?name=='$1'].[resourceGroup]"`
}

#
# __cloud_FindInstances
#
# Find instances matching the specified pattern.
#
# For each matching instance, an entry in the `instances` array will be added with the
# following information about the instance:
#   "name:public IP:private IP:location"
#
# filter   - The instances to filter on
#
# examples:
#   $ __cloud_FindInstances prefix some-machine-prefix
#   $ __cloud_FindInstances name exact-machine-name
#
#  Examples of plain-text filter command
#
#  This will return an exact match for a machine named pgnode
#  az vm list -d --query "[?name=='pgnode'].[name,publicIps,privateIps,location]"
#
#  This will return a match for any machine with prefix pgnode, ex: pgnode and pgnode2
#  az vm list -d --query "[?starts_with(name,'pgnode')].[name,publicIps,privateIps,location]"

__cloud_FindInstances() {

  case $1 in
    prefix)
      query="[?starts_with(name,'$2')]"
      ;;
    name)
      query="[?name=='$2']"
      ;;
    *)
      echo "Unknown filter command: $1"
      ;;
  esac

  keys="[name,publicIps,privateIps,location]"

  while read -r name publicIp privateIp location; do
    instances+=("$name:$publicIp:$privateIp:$location")
  done < <(az vm list -d -o tsv --query "$query.$keys")
  echo ${instances[*]}
}

#
# cloud_FindInstances [namePrefix]
#
# Find instances with names matching the specified prefix
#
# For each matching instance, an entry in the `instances` array will be added with the
# following information about the instance:
#   "name:public IP:private IP:location"
#
# namePrefix - The instance name prefix to look for
#
# examples:
#   $ cloud_FindInstances all-machines-with-a-common-machine-prefix
#
cloud_FindInstances() {
  __cloud_FindInstances prefix $1
}

#
# cloud_FindInstance [name]
#
# Find an instance with a name matching the exact pattern.
#
# For each matching instance, an entry in the `instances` array will be added with the
# following information about the instance:
#   "name:public IP:private IP:location"
#
# name - The instance name to look for
#
# examples:
#   $ cloud_FindInstance exact-machine-name
#
cloud_FindInstance() {
  __cloud_FindInstances name $1
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
  echo "TODO: create $networkName firewall rules programmatically instead of assuming the 'testnet' tag exists"
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
  declare enableGpu="$4"
  declare machineType="$5"
  declare zone="$6"
  declare optionalBootDiskSize="$7"
  declare optionalStartupScript="$8"
  declare optionalAddress="$9"
  declare optionalBootDiskType="${10}"

  if $enableGpu; then
    # TODO: First out what image is needed for Azure for GPU/non-GPU
    imageName="UbuntuLTS"
  else
    # TODO: First out what image is needed for Azure for GPU/non-GPU
    imageName="UbuntuLTS"
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
    --resource-group $networkName
    --tags testnet
    --image $imageName
    --size $machineType
    --location $zone
    --generate-ssh-keys
  )

  if [[ -n $optionalBootDiskSize ]]; then
    args+=(
      --os-disk-size-gb "$optionalBootDiskSize"
    )
  fi
  if [[ -n $optionalStartupScript ]]; then
    args+=(
      --custom-data "$optionalStartupScript"
    )
  fi

  if [[ -n $optionalBootDiskType ]]; then
    echo Boot disk type not configurable
  fi

  if [[ -n $optionalAddress ]]; then
    [[ $numNodes = 1 ]] || {
      echo "Error: address may not be supplied when provisioning multiple nodes: $optionalAddress"
      exit 1
    }
    args+=(
      --public-ip-address "$optionalAddress"
    )
  fi

  (
  set -x
    # 1: Check if resource group exists.  If not, create it.
    numGroup=`az group list --query "length([?name=='$networkName'])"`
    if [[ $numGroup -eq 0 ]]; then
      echo Resource Group "$networkName" does not exist.  Creating it now.
      az group create --name $networkName --location $zone
    else
      echo Resource group "$networkName" already exists.
      az group show --name $networkName
    fi

    # 2: For node in numNodes, create VM
    for nodeName in ${nodes[@]}; do
      az vm create --name $nodeName "${args[@]}" --verbose
    done
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
  declare locations=("${instances[@]/*:/}")

  (
    set -x
    for instance in ${names[@]}; do
      cloud_GetResourceGroupFromInstance $instance
      az vm delete --name $instance --resource-group $resourceGroup --no-wait --yes --verbose
    done
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
  declare zone="$5"

  cloud_GetConfigValueFromInstanceName "$nodeName" osProfile.adminUsername
  scp "$config_value"@"$publicIp:$remoteFile" "$localFile"

}
