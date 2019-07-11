#!/usr/bin/env bash
set -x

mount_point=/mnt/extra-disk
disk=sdb
if ! lsblk | grep -q ${disk} ; then
  echo "${disk} does not exist"
else
  if mount | grep -q ${disk} ; then
    echo "${disk} is already mounted"
  else
    sudo mkfs.ext4 -F /dev/"$disk"
    sudo mkdir -p /mnt/disks/"$disk"
    sudo mount /dev/"$disk" "$mount_point"
    sudo chmod a+w /mnt/disks/"$disk"
    if ! mount | grep -q ${disk} ; then
      echo "${disk} failed to mount!"
      exit 1
    fi
  fi
fi
