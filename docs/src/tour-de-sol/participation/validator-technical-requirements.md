# Requirements to run a validator

## Hardware

See [suggested hardware configuration here](../../running-validator/validator-reqs.md).

* CPU Recommendations
  * We recommend a CPU with the highest number of cores as possible. AMD Threadripper or Intel Server \(Xeon\) CPUs are fine.
  * We recommend AMD Threadripper as you get a larger number of cores for parallelization compared to Intel.
  * Threadripper also has a cost-per-core advantage and a greater number of PCIe lanes compared to the equivalent Intel part. PoH \(Proof of History\) is based on sha256 and Threadripper also supports sha256 hardware instructions.
* SSD size and I/O style \(SATA vs NVMe/M.2\)    for a validator
  * Minimum example - Samsung 860 Evo 2TB
  * Mid-range example - Samsung 860 Evo 4TB
  * High-end example - Samsung 860 Evo 4TB
* GPUs
  * **Validator** nodes will be required to run with GPUs starting at Stage 1 of Tour de SOL. Without GPUs, a validator will not be able to catch up to the ledger once the network is launched. GPUs are NOT required for validators during Stage 0/Dry Runs of Tour de SOL.
  * What kind of GPU?
    * We recommend Nvidia 2080Ti or 1080Ti series consumer GPU or Tesla series server GPUs.
    * We do not currently support OpenCL and therefore do not support AMD GPUs.  We have a bounty out for someone to port us to OpenCL. Interested? [Check out our GitHub.](https://github.com/solana-labs/solana)
* Power Consumption
  * Approximate power consumption for a validator node running an AMD Threadripper 2950W and 2x 2080Ti GPUs is 800-1000W.

## **Software**

* We build and run on Ubuntu 18.04.  Some users have had trouble when running on Ubuntu 16.04
* See [Connecting Your Validator](steps-to-create-a-validator/connecting-your-validator.md#install-software) for the current Solana software release.
