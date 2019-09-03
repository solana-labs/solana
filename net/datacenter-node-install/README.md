# Introduction

These scripts are intended to facilitate the preparation of dedicated Solana
nodes.  They have been tested as working from a clean installation of Ubuntu
18.04 Server.  Use elsewhere is unsupported.

# Installation

Both installation methods require that the NVIDIA proprietary driver installer
programs be downloaded alongside [setup-cuda.sh](./setup-cuda.sh). If they do
not exist at runtime, an attempt will be made to download them automatically. To
avoid downloading the installers at runtime, they may be downloaded in advance
and placed as siblings to [setup-cuda.sh](./setup-cuda.sh).

For up-to-date NVIDIA driver version requirements, see [setup-cuda.sh](./setup-cuda.sh)

## Datacenter Node

1) `sudo ./setup-dc-node-1.sh`
2) `sudo reboot`
3) `sudo ./setup-dc-node-2.sh`

## Partner Node

1) `$ sudo ./setup-partner-node.sh`
