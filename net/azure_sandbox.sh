#!/usr/bin/env bash

set -e
set -x

usage() {
  exitcode=0
  if [[ -n "$1" ]]; then
    exitcode=1
    echo "Error: $*"
  fi
  cat <<EOF
usage: $0 [create|config|info|delete]

Manage testnet instances

 create - create a new testnet (implies 'config')
 config - configure the testnet and write a config file describing it
 delete - delete the testnet
 info   - display information about the currently configured testnet

EOF
  exit $exitcode
}

command=$1
[[ -n $command ]] || usage
shift

[[ $command = create || $command = config || $command = info || $command = delete ]] ||
  usage "Invalid command: $command"

while getopts "h?g:n:l:" opt; do
  case $opt in
  h | \?)
    usage
    ;;
  g)
    resourceGroup=$OPTARG
    ;;
  n)
    vmName=$OPTARG
    ;;
  l)
    location=$OPTARG
    ;;
  *)
    usage "unhandled option: $opt"
    ;;
  esac
done

image=UbuntuLTS
diskSizeGB=20
size=Standard_D16s_v3

create() {
  # See if the group exists.  If not, create it
  numGroup=`az group list --query "length([?name=='$resourceGroup'])"`
  if [[ $numGroup -eq 0 ]]; then
    echo Group "$resourceGroup" does not exist.  Creating it now.
    az group create -n $resourceGroup -l $location
  else
    echo Group "$resourceGroup" already exists.
    az group show -n $resourceGroup
  fi
  echo Creating VM "$vmName"
  az vm create --name $vmName --resource-group $resourceGroup --image $image \
  --data-disk-sizes-gb $diskSizeGB --size $size \
  --generate-ssh-keys


}

delete() {
  az group delete -g $resourceGroup -y
}

case $command in
delete)
  delete
  ;;
create)
  create
  ;;
*)
  usage "Unknown command: $command"
esac

filter() {

az vm list --query "length([?starts_with(name, 'pgnode')])"
az vm list --query "length([?contains(name, 'c')])"

}