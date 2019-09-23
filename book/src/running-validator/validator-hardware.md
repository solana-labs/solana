# Hardware Requirements

Since the testnet is not intended for stress testing of max transaction throughput, a higher-end machine with a GPU is not necessary to participate.

However ensure the machine used is not behind a residential NAT to avoid NAT traversal issues. A cloud-hosted machine works best. **Ensure that IP ports 8000 through 10000 are not blocked for Internet inbound and outbound traffic.**

Prebuilt binaries are available for Linux x86\_64 \(Ubuntu 18.04 recommended\). MacOS or WSL users may build from source.

## Recommended Setups

For a performance testnet with many transactions we have some preliminary recommended setups:

|  | Low end | Medium end | High end | Notes |
| :--- | :--- | :--- | :--- | :--- |
| CPU | AMD Threadripper 1900x | AMD Threadripper 2920x | AMD Threadripper 2950x | Consider a 10Gb-capable motherboard with as many PCIe lanes and m.2 slots as possible. |
| RAM | 16GB | 32GB | 64GB |  |
| OS Drive | Samsung 860 Evo 2TB | Samsung 860 Evo 4TB | Samsung 860 Evo 4TB | Or equivalent SSD |
| Accounts Drive\(s\) | None | Samsung 970 Pro 1TB | 2x Samsung 970 Pro 1TB |  |
| GPU | 4x Nvidia 1070 or 2x Nvidia 1080 Ti or 2x Nvidia 2070 | 2x Nvidia 2080 Ti | 4x Nvidia 2080 Ti | Any number of cuda-capable GPUs are supported on Linux platforms. |

## GPU Requirements

CUDA is required to make use of the GPU on your system. The provided Solana release binaries are built on Ubuntu 18.04 with [CUDA Toolkit 10.1 update 1"](https://developer.nvidia.com/cuda-toolkit-archive). If your machine is using a different CUDA version then you will need to rebuild from source.

