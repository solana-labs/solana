# |source| this file
#
# Utilities for working with EC2 instances
#

cloud_DefaultZone() {
  echo "us-east-1b"
}

cloud_RestartPreemptedInstances() {
  : # Not implemented
}

# AWS region is zone with the last character removed
__cloud_GetRegion() {
  declare zone="$1"
  # AWS region is zone with the last character removed
  declare region="${zone:0:$((${#zone} - 1))}"
  echo "$region"
}

# sshPrivateKey should be globally defined whenever this function is called.
#
# TODO: Remove usage of the sshPrivateKey global
__cloud_SshPrivateKeyCheck() {
  # shellcheck disable=SC2154
  if [[ -z $sshPrivateKey ]]; then
    echo Error: sshPrivateKey not defined
    exit 1
  fi
  if [[ ! -r $sshPrivateKey ]]; then
    echo "Error: file is not readable: $sshPrivateKey"
    exit 1
  fi
}

#
# __cloud_FindInstances
#
# Find instances with name matching the specified pattern.
#
# For each matching instance, an entry in the `instances` array will be added with the
# following information about the instance:
#   "name:public IP:private IP"
#
# filter   - The instances to filter on
#
# examples:
#   $ __cloud_FindInstances "exact-machine-name"
#   $ __cloud_FindInstances "all-machines-with-a-common-machine-prefix*"
#
__cloud_FindInstances() {
  declare filter="$1"

  instances=()
  declare -a regions=("us-east-1" "us-east-2" "us-west-1" "us-west-2" "sa-east-1" "ap-northeast-2" \
   "ap-northeast-1" "ap-southeast-2" "ap-southeast-1" "ap-south-1" "eu-west-1" "eu-west-2" "eu-central-1" "ca-central-1")
  for region in "${regions[@]}"
  do
    declare name publicIp privateIp
    while read -r name publicIp privateIp zone; do
      printf "%-30s | publicIp=%-16s privateIp=%s zone=%s\n" "$name" "$publicIp" "$privateIp" "$zone"
      instances+=("$name:$publicIp:$privateIp:$zone")
    done < <(aws ec2 describe-instances \
              --region "$region" \
              --filters \
                "Name=tag:name,Values=$filter" \
                "Name=instance-state-name,Values=pending,running" \
              --query "Reservations[].Instances[].[InstanceId,PublicIpAddress,PrivateIpAddress,Placement.AvailabilityZone]" \
              --output text \
      )
  done
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
  __cloud_FindInstances "$namePrefix*"
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
  __cloud_FindInstances "$name"
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
  declare zone="$2"
  declare region=
  region=$(__cloud_GetRegion "$zone")

  __cloud_SshPrivateKeyCheck
  aws ec2 delete-key-pair --region "$region" --key-name "$networkName"
  aws ec2 import-key-pair --region "$region" --key-name "$networkName" \
    --public-key-material file://"${sshPrivateKey}".pub

  declare rules
  rules=$(cat "$(dirname "${BASH_SOURCE[0]}")"/ec2-security-group-config.json)
  aws ec2 delete-security-group --region "$region" --group-name "$networkName" || true
  aws ec2 create-security-group --region "$region" --group-name "$networkName" --description "Created automatically by $0"
  aws ec2 authorize-security-group-ingress --output table --region "$region" --group-name "$networkName" --cli-input-json "$rules"
}

#
# cloud_CreateInstances [networkName] [namePrefix] [numNodes] [imageName]
#                       [machineType] [bootDiskSize] [startupScript] [address]
#
# Creates one more identical instances.
#
# networkName   - unique name of this testnet
# namePrefix    - unique string to prefix all the instance names with
# numNodes      - number of instances to create
# imageName     - Disk image for the instances
# machineType   - GCE machine type
# bootDiskSize  - Optional size of the boot disk in GB
# startupScript - Optional startup script to execute when the instance boots
# address       - Optional name of the GCE static IP address to attach to the
#                 instance.  Requires that |numNodes| = 1 and that addressName
#                 has been provisioned in the GCE region that is hosting |zone|
# bootDiskType  - Optional specify SSD or HDD boot disk
# additionalDiskSize - Optional specify size of additional storage volume
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
  declare region=
  region=$(__cloud_GetRegion "$zone")

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
      imageName="ami-0fba9b33b5304d8b4"
      ;;
    us-east-2)
      imageName="ami-0e04554247365d806"
      ;;
    us-west-1)
      imageName="ami-07390b6ff5934a238"
      ;;
    us-west-2)
      imageName="ami-03804ed633fe58109"
      ;;
    sa-east-1)
      imageName="ami-0f1678b6f63a0f923"
      ;;
    ap-northeast-2)
      imageName="ami-0695e34e31339c3ff"
      ;;
    ap-northeast-1)
      imageName="ami-003371bfa26192744"
      ;;
    ap-southeast-2)
      imageName="ami-0401c9e2f645b5557"
      ;;
    ap-southeast-1)
      imageName="ami-08050c889a630f1bd"
      ;;
    ap-south-1)
      imageName="ami-04184c12996409633"
      ;;
    eu-central-1)
      imageName="ami-054e21e355db24124"
      ;;
    eu-west-1)
      imageName="ami-0727f3c2d4b0226d5"
      ;;
    eu-west-2)
      imageName="ami-068f09e337d7da0c4"
      ;;
    ca-central-1)
      imageName="ami-06ed08059bdc08fc9"
      ;;
    *)
      usage "Unsupported region: $region"
      ;;
    esac
  fi

  declare -a args
  args=(
    --key-name "$networkName"
    --count "$numNodes"
    --region "$region"
    --placement "AvailabilityZone=$zone"
    --security-groups "$networkName"
    --image-id "$imageName"
    --instance-type "$machineType"
    --tag-specifications "ResourceType=instance,Tags=[{Key=name,Value=$namePrefix}]"
  )
  if [[ -n $optionalBootDiskSize ]]; then
    args+=(
      --block-device-mapping "[{\"DeviceName\": \"/dev/sda1\", \"Ebs\": { \"VolumeSize\": $optionalBootDiskSize }}]"
    )
  fi
  if [[ -n $optionalStartupScript ]]; then
    args+=(
      --user-data "file://$optionalStartupScript"
    )
  fi

  if [[ -n $optionalAddress ]]; then
    [[ $numNodes = 1 ]] || {
      echo "Error: address may not be supplied when provisioning multiple nodes: $optionalAddress"
      exit 1
    }
  fi

  (
    set -x
    aws ec2 run-instances --output table "${args[@]}"
  )

  if [[ -n $optionalAddress ]]; then
    cloud_FindInstance "$namePrefix"
    if [[ ${#instances[@]} -ne 1 ]]; then
      echo "Failed to find newly created instance: $namePrefix"
    fi

    declare instanceId
    IFS=: read -r instanceId publicIp privateIp zone < <(echo "${instances[0]}")
    (
      set -x
      # TODO: Poll that the instance has moved to the 'running' state instead of
      #       blindly sleeping for 30 seconds...
      sleep 30
      declare region=
      region=$(__cloud_GetRegion "$zone")
      aws ec2 associate-address \
        --instance-id "$instanceId" \
        --region "$region" \
        --allocation-id "$optionalAddress"
    )
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

  # Terminate the instances
  for instance in "${instances[@]}"; do
    declare name="${instance/:*/}"
    declare zone="${instance/*:/}"
    declare region=
    region=$(__cloud_GetRegion "$zone")
    (
      set -x
      aws ec2 terminate-instances --output table --region "$region" --instance-ids "$name"
    )
  done

  # Wait until the instances are terminated
  for instance in "${instances[@]}"; do
    declare name="${instance/:*/}"
    declare zone="${instance/*:/}"
    declare region=
    region=$(__cloud_GetRegion "$zone")
    while true; do
      declare instanceState
      instanceState=$(\
        aws ec2 describe-instances \
          --region "$region" \
          --instance-ids "$name" \
          --query "Reservations[].Instances[].State.Name" \
          --output text \
      )
      echo "$name: $instanceState"
      if [[ $instanceState = terminated ]]; then
        break;
      fi
      sleep 2
    done
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
#  declare instanceZone="$3"  # unused
  declare timeout="$4"

  timeout "${timeout}"s bash -c "set -o pipefail; until ping -c 3 $instanceIp | tr - _; do echo .; done"
}

#
# cloud_FetchFile [instanceName] [publicIp] [remoteFile] [localFile]
#
# Fetch a file from the given instance.  This function uses a cloud-specific
# mechanism to fetch the file
#
cloud_FetchFile() {
  # shellcheck disable=SC2034 # instanceName is unused
  declare instanceName="$1"
  declare publicIp="$2"
  declare remoteFile="$3"
  declare localFile="$4"

  __cloud_SshPrivateKeyCheck
  (
    set -x
    scp \
      -o "StrictHostKeyChecking=no" \
      -o "UserKnownHostsFile=/dev/null" \
      -o "User=solana" \
      -o "IdentityFile=$sshPrivateKey" \
      -o "LogLevel=ERROR" \
      -F /dev/null \
      "solana@$publicIp:$remoteFile" "$localFile"
  )
}

#
# cloud_CreateAndAttachPersistentDisk
#
# Not yet implemented for this cloud provider
cloud_CreateAndAttachPersistentDisk() {
  echo "ERROR: cloud_CreateAndAttachPersistentDisk is not yet implemented for ec2"
  exit 1
}

#
# cloud_StatusAll
#
# Not yet implemented for this cloud provider
cloud_StatusAll() {
  echo "ERROR: cloud_StatusAll is not yet implemented for ec2"
}
