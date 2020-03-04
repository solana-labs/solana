# Validator Requirements

## Hardware

* CPU Recommendations
  * We recommend a CPU with the highest number of cores as possible. AMD Threadripper or Intel Server \(Xeon\) CPUs are fine.
  * We recommend AMD Threadripper as you get a larger number of cores for parallelization compared to Intel.
  * Threadripper also has a cost-per-core advantage and a greater number of PCIe lanes compared to the equivalent Intel part. PoH \(Proof of History\) is based on sha256 and Threadripper also supports sha256 hardware instructions.
* SSD size and I/O style \(SATA vs NVMe/M.2\) for a validator
  * Minimum example - Samsung 860 Evo 2TB
  * Mid-range example - Samsung 860 Evo 4TB
  * High-end example - Samsung 860 Evo 4TB
* GPUs
  * While a CPU-only node may be able to keep up with the initial idling network, once transaction throughput increases, GPUs will be necessary
  * What kind of GPU?
    * We recommend Nvidia 2080Ti or 1080Ti series consumer GPU or Tesla series server GPUs.
    * We do not currently support OpenCL and therefore do not support AMD GPUs. We have a bounty out for someone to port us to OpenCL. Interested? [Check out our GitHub.](https://github.com/solana-labs/solana)
* Power Consumption
  * Approximate power consumption for a validator node running an AMD Threadripper 2950W and 2x 2080Ti GPUs is 800-1000W.

### Preconfigured Setups

Here are our recommendations for low, medium, and high end machine specifications:

|  | Low end | Medium end | High end | Notes |
| :--- | :--- | :--- | :--- | :--- |
| CPU | AMD Threadripper 1900x | AMD Threadripper 2920x | AMD Threadripper 2950x | Consider a 10Gb-capable motherboard with as many PCIe lanes and m.2 slots as possible. |
| RAM | 16GB | 32GB | 64GB |  |
| OS Drive | Samsung 860 Evo 2TB | Samsung 860 Evo 4TB | Samsung 860 Evo 4TB | Or equivalent SSD |
| Accounts Drive\(s\) | None | Samsung 970 Pro 1TB | 2x Samsung 970 Pro 1TB |  |
| GPU | 4x Nvidia 1070 or 2x Nvidia 1080 Ti or 2x Nvidia 2070 | 2x Nvidia 2080 Ti | 4x Nvidia 2080 Ti | Any number of cuda-capable GPUs are supported on Linux platforms. |

## Software

* We build and run on Ubuntu 18.04.  Some users have had trouble when running on Ubuntu 16.04
* See [Installing Solana](../install-solana.md) for the current Solana software release.

Be sure to ensure that the machine used is not behind a residential NAT to avoid
NAT traversal issues. A cloud-hosted machine works best. **Ensure that IP ports 8000 through 10000 are not blocked for Internet inbound and outbound traffic.**
For more information on port forwarding with regards to residential networks,
see [this document](http://www.mcs.sdsmt.edu/lpyeatt/courses/314/PortForwardingSetup.pdf).

Prebuilt binaries are available for Linux x86\_64 \(Ubuntu 18.04 recommended\).
MacOS or WSL users may build from source.

## GPU Requirements

CUDA is required to make use of the GPU on your system. The provided Solana
release binaries are built on Ubuntu 18.04 with [CUDA Toolkit 10.1 update 1](https://developer.nvidia.com/cuda-toolkit-archive). If your machine is using
a different CUDA version then you will need to rebuild from source.
