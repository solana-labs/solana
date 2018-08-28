# |source| this file
#
# Utilities for working with gcloud
#


#
# gcloud_FindInstances [filter]
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
#   $ gcloud_FindInstances "name=exact-machine-name"
#   $ gcloud_FindInstances "name~^all-machines-with-a-common-machine-prefix"
#
gcloud_FindInstances() {
  declare filter="$1"
  #gcloud compute instances list --filter="$filter"
  instances=()
  while read -r name zone publicIp privateIp status; do
    if [[ $status != RUNNING ]]; then
      echo "Warning: $name is not RUNNING, ignoring it."
      continue
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

    eval "$cmd" "$name" "$zone" "$publicIp" "privateIp" "$count" "$@"
    count=$((count + 1))
  done
}

#
# gcloud_CreateInstances [namePrefix] [numNodes] [zone] [imageName]
#
# Creates one more identical instances.
#
# namePrefix   - unique string to prefix all the instance names with
# numNodes     - number of instances to create
# zone         - zone to create the instances in
# imageName    - Disk image for the instances
#
# Tip: use gcloud_FindInstances to locate the instances once this function
#      returns
gcloud_CreateInstances() {
  declare namePrefix="$1"
  declare numNodes="$2"
  declare zone="$3"
  declare imageName="$4"

  declare nodes
  read -ra nodes <<<$(seq -f "${namePrefix}%g" 1 "$numNodes")

  (
    set -x
    gcloud beta compute instances create "${nodes[@]}" \
      --zone="$zone" \
      --tags=testnet \
      --image="$imageName"
  )
}

#
# gcloud_DeleteInstances
#
# Deletes all the instances listed in the `instances` array
#
gcloud_DeleteInstances() {
  declare names=("${instances[@]/:*/}")
  (
    set -x
    gcloud beta compute instances delete "${names[@]}"
  )
}

#
# gcloud_FigureRemoteUsername [instanceInfo]
#
# The remote username when ssh-ing into GCP instances tends to not be the same
# as the user's local username, but it needs to be discovered by ssh-ing into an
# instance and examining the system.
#
# On success the gcloud_username global variable is updated
#
# instanceInfo  - an entry from the `instances` array
#
# example:
#   gcloud_FigureRemoteUsername "name:zone:..."
#
gcloud_FigureRemoteUsername() {
  if [[ -n $gcloud_username ]]; then
    return
  fi

  declare instanceInfo="$1"
  declare name zone
  IFS=: read -r name zone _ < <(echo "$instanceInfo")

  echo "Detecting remote username using $zone in $zone:"
  # Figure the gcp ssh username
  (
    set -x
    gcloud compute ssh "$name" --zone "$zone" -- "echo whoami \$(whoami)" | tee whoami
  )

  [[ "$(tr -dc '[:print:]' < whoami; rm -f whoami)" =~ ^whoami\ (.*)$ ]] || {
    echo Unable to figure remote user name;
    exit 1
  }
  gcloud_username="${BASH_REMATCH[1]}"
  echo "Remote username: $gcloud_username--"
}

#
# gcloud_PrepInstancesForSsh [username] [publicKey] [privateKey]
#
# Prepares all the instances in the `instances` array for ssh with the specified
# keypair.  This eliminates the need to use the restrictive |gcloud compute ssh|,
# use plain |ssh| instead.
#
# username    - gcp ssh username as computed by gcloud_FigureRemoteUsername
# publicKey   - public key to install on all the instances
# privateKey  - matching private key, used to verify ssh access
#
gcloud_PrepInstancesForSsh() {
  declare username="$1"
  declare publicKey="$2"
  declare privateKey="$3"
  [[ -r $publicKey ]] || {
    echo "Unable to read public key: $publicKey"
    exit 1
  }

  [[ -r $privateKey ]] || {
    echo "Unable to read private key: $privateKey"
    exit 1
  }

  for instanceInfo in "${instances[@]}"; do
    declare name zone publicIp
    IFS=: read -r name zone publicIp _ < <(echo "$instanceInfo")
    (
      set -x

      # TODO: stomping on the authorized_keys isn't great, maybe do something
      #       clever with |ssh-copy-id| one day
      gcloud compute scp --zone "$zone" "$publicKey" "$name":.ssh/authorized_keys

      # Confirm normal ssh now works
      ssh -i "$privateKey" "$username@$publicIp" uptime
    )
  done
}

