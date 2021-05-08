---
title: 验证节点要求
---

## 硬件

- CPU 推荐
  - 我们建议使用尽可能多的内核。 AMD Threadripper或Intel服务器\(Xeon\) CPU都很好。
  - 我们建议您使用AMD Threadripper，因为与英特尔相比，您可以获得更多的并行内核。
  - 与同等的英特尔产品相比，Threadripper还具有每核成本优势和更多的PCIe通道。 PoH\(历史证明\) 基于sha256，并且Threadripper还支持sha256硬件指令。
- 验证节点的SSD规则I/O样式\(SATA vs NVMe/M.2\)
  - 最低配置示例 - Samsung 860 Evo 2TB
  - 中等配置示例 - Samsung 860 Evo 4TB
  - 高端配置示例 - Samsung 860 Evo 4TB
- GPU
  - 虽然纯CPU的节点可能能够跟上初始的空闲网络，但是一旦事务吞吐量增加，就将需要GPU。
  - 什么类型的 GPU ？
    - 我们建议使用Nvidia Turing和volta系列GPU（从1660ti到2080ti系列用户级GPU或Tesla系列服务器GPU）。
    - 我们目前不支持OpenCL，因此不支持AMD GPU。 我们对移植Solana到OpenCL设有一项奖励。 有兴趣？ [请查看我们的GitHub。](https://github.com/solana-labs/solana)
- 电源消耗
  - 运行AMD Threadripper 3950x和2x 2080Ti GPU的验证器节点，其功耗大约为800-1000W。

### 预配置设置

以下是我们关于低、中、高端机器规格的建议：

|             | 低端配置                | 中等配置                   | 高端配置                   | 注意事项                                   |
|:----------- |:------------------- |:---------------------- |:---------------------- |:-------------------------------------- |
| CPU         | AMD Ryzen 3950x     | AMD Threadripper 3960x | AMD Threadripper 3990x | 考虑设立一个能够使用10Gb的主板，尽可能多地配备PCIe通道和m.2插槽。 |
| RAM         | 32GB                | 64GB                   | 128GB                  |                                        |
| Ledger 驱动器  | Samsung 860 Evo 2TB | Samsung 860 Evo 4TB    | Samsung 860 Evo 4TB    | 或等效的 SSD                               |
| 账户驱动\(s\) | 无                   | Samsung 970 Pro 1TB    | 2x Samsung 970 Pro 1TB |                                        |
| GPU         | Nvidia 1660ti       | Nvidia 2080 Ti         | 2x Nvidia 2080 Ti      | Linux 平台支持的任何 Cuda 的 GPU。              |

## 云平台虚拟机

虽然您可以在云计算平台上运行验证节点，但从长远来看它可能并不具有成本效益。

但是，在VM实例上运行非投票api节点，来供您自己内部使用可能会很方便。 该用例包括在Solana上构建的交易所和服务。

实际上，官方mainnet-beta API节点当前(2020年10月) 在具有2048 GB SSD的 GCE `n1-standard-32`(32 vCPU，120 GB内存) 实例上运行。

对于其他云平台，请选择具有类似规格的实例类型。

并且还要注意，出口互联网流量使用可能会很高，尤其是运行质押验证程序的情况。

## Docker

我们不建议在Docker内部为活动集群(包括mainnet-beta) 运行验证程序，并且通常也不支持这个功能。 除非特别配置，否则这是由于担心一般docker的容器化开销和导致的性能下降。

我们仅将docker用于开发目的。

## 软件设置

- 我们在Ubuntu 04/18上进行开发和运行。 在Ubuntu 04/16上运行时，某些用户会遇到一些问题
- 请参阅 [安装Solana](../cli/install-solana-cli-tools.md) 以获取当前的 Solana 软件版本。

请确保所使用的计算机不在本地NAT后面，以避免NAT遍历问题。 云托管的机器效果最好。 **请确保8000到10000的IP端口没有被禁止，来让网络实现进出通信。** 关于寄宿网络端口转发的更多信息，请参阅 [这个文档](http://www.mcs.sdsmt.edu/lpyeatt/courses/314/PortForwardingSetup.pdf)。

预构建的二进制文件是在 Linux x86_64 \(推荐Ubuntu 18.04\)。 MacOS 或 WSL 用户可以从源代码构建。

## GPU 要求

要在您的系统上使用GPU，必须使用CUDA。 发布的 Solana 二进制文件是通过 [CUDA Toolkit 10.1 update 1](https://developer.nvidia.com/cuda-toolkit-archive) 在 Ubuntu 18.04 上构建的。 如果您的机器使用了不同的 CUDA，那么您将需要从源代码进行重建。
