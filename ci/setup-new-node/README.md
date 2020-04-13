# Introduction

These scripts are intended to facilitate the preparation of dedicated Solana
nodes to be used for testing and/or Buildkite-based CI.

# Pre-Requisites

 - Install Ubuntu 18.04 LTS Server
 - Log in as a local or remote user with `sudo` privileges

# Install Core Requirements

#### Non-GPU enabled machines
```bash
sudo ./setup-new-node.sh
```

#### GPU-enabled machines
 - 1 or more NVIDIA GPUs should be installed in the machine (tested with 2080Ti)
```bash
export CUDA=1
sudo ./setup-new-node.sh
```

# Configure Node for CI

1) Install `buildkite-agent` and set up it user environment with:
```bash
sudo ./setup-buildkite.sh
```
2) Copy the pubkey contents from `~buildkite-agent/.ssh/id_ecdsa.pub`
3) Add the pubkey as an authorized SSH key on github
4) Edit `/etc/buildkite-agent/buildkite-agent.cfg` and/or `/etc/systemd/system/buildkite-agent@*` to the desired configuration of the agent(s)
5) Copy `ejson` keys from another CI node at `/opt/ejson/keys/`
to the same location on the new node.

6) Start the new agent(s) with `sudo systemctl enable --now buildkite-agent`