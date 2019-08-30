# Introduction

These scripts are intended to facilitate the preparation of dedicated Solana
nodes.  They have been tested as working from a clean installation of Ubuntu
18.04 Server.  Use elsewhere is unsupported.

# Installation

Both installation methods require that the scripts be run from the same
directory as the NVIDIA proprietary driver installer programs have been
downloaded to. `.` is `net/datacenter-node-install` in these examples, so the
NVIDIA installers should be downloaded there as well. For up-to-date NVIDIA
driver version requirements, see [./setup-cuda.sh](setup-cuda.sh.)

## Datacenter Node

1) `sudo ./setup-dc-node-1.sh`
2) `sudo reboot`
3) `sudo ./setup-dc-node-2.sh`

## Partner Node

1) `$ sudo ./setup-partner-node.sh`
