#!/usr/bin/env bash
set -e
set -x

# Create ext4 filesystem and mount all nvme-type SSDs

ssd_list="$(lsblk | grep nvme | cut -f 1 -d " ")"

for ssd in $ssd_list ; do
  sudo mkfs.ext4 -F /dev/"$ssd"
  sudo mkdir -p /mnt/disks/"$ssd"
  sudo mount /dev/"$ssd" /mnt/disks/"$ssd"
  sudo chmod a+w /mnt/disks/"$ssd"
done

df -h /mnt/disks/*
