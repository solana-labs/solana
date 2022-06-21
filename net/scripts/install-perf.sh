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
# Impose no scope and access restrictions on using perf_events performance monitoring
echo -1 | tee /proc/sys/kernel/perf_event_paranoid
# Allow recording kernel reference relocation symbol to avoid skewing symbol resolution if relocation was used
echo 0 | tee /proc/sys/kernel/kptr_restrict
