#!/usr/bin/env bash
#
# perf setup
#
set -ex

[[ $(uname) = Linux ]] || exit 1
[[ $USER = root ]] || exit 1

# install perf
apt-get --assume-yes install linux-tools-common linux-tools-generic "linux-tools-$(uname -r)"

# setup permissions
echo -1 | tee /proc/sys/kernel/perf_event_paranoid
sh -c "echo 0 > /proc/sys/kernel/kptr_restrict"
