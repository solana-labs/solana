#!/usr/bin/env bash
set -x

disk=sdb
if [[ -z $(lsblk | grep ${disk}) ]]; then
  echo "${disk} does not exist"
else
  if [[ -n $(mount | grep ${disk}) ]]; then
    echo "${disk} is already mounted"
  else
    sudo mkfs.ext4 -F /dev/"$disk"
    sudo mkdir -p /mnt/disks/"$disk"
    sudo mount /dev/"$disk" /mnt/disks/"$disk"
    sudo chmod a+w /mnt/disks/"$disk"
    if [[ -z $(mount | grep ${disk}) ]]; then
      echo "${disk} failed to mount!"
      exit 1
    fi
  fi
fi
