---
title: 连接到一个集群
---

查看 [Solana 集群](../clusters.md) 获取关于可用集群的通用信息。

## 配置命令行工具

您可以通过下述指令来检查集群正在运行的 Solana 命令行工具 (CLI)：

```bash
solana config get
```

通过 `solana config set` 命令来制定某个集群。 设置一个目标集群后，未来的任何子命令都会从该集群发送/接收信息。

例如，要指定 Devnet 集群，请运行：

```bash
solana config set --url https://api.devnet.solana.com
```

## 确保版本相匹配

虽然严格来说没有必要，但是当 CLI 版本与运行在集群中的软件版本相匹配时，一般来说 CLI 版本能够发挥最大作用。 查看本地安装的 CLI 版本，请运行：

```bash
solana --version
```

查看集群版本，请运行：

```bash
solana cluster-version
```

确保本地的 CLI 版本比群集版本新或者至少相同。
