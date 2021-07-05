#!/usr/bin/env bash
set -x

mount_point=/mnt/extra-disk
disk=sdb
if ! lsblk | grep -q ${disk} ; then
  echo "${disk} does not exist"
else
  sudo mount /dev/"$disk" "$mount_point" || true
  if mount | grep -q ${disk} ; then
    echo "${disk} is mounted"
  else
    sudo mkfs.ext4 -F /dev/"$disk"
    sudo mkdir -p "$mount_point"
    sudo mount /dev/"$disk" "$mount_point"
    sudo chmod a+w "$mount_point"
    if ! mount | grep -q ${mount_point} ; then
      echo "${disk} failed to mount!"
      exit 1
    fi
  fi
fi
