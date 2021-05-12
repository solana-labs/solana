---
title: Solana 集群
---

Solana维护着几个不同用途的集群。

在开始之前，请确保已首先安装了[Solana命令行工具](cli/install-solana-cli-tools.md)

浏览器：

- [http://explorer.solana.com/](https://explorer.solana.com/)。
- [http://solanabeach.io/](http://solanabeach.io/).

## Devnet（开发者网络）

- Devnet可以作为希望将Solana进行测试的任何人，用户，代币持有者，应用开发者或验证者的游乐场。
- 应用程序开发人员应针对Devnet。
- 潜在的验证者应首先针对Devnet。
- Devnet和Mainnet Beta之间的主要区别：
  - Devnet代币是**不是真实的**
  - Devnet包含用于空投的代币龙头，用于应用程序测试
  - Devnet可能会重置账本
  - Devnet通常运行比Mainnet Beta更新的软件版本
- Devnet的八卦入口点：`entrypoint.devnet.solana.com：8001`
- Devnet的指标环境变量：
```bash
export SOLANA_METRICS_CONFIG="host=https://metrics.solana.com:8086,db=devnet,u=scratch_writer,p=topsecret"
```
- Devnet RPC URL：`https://devnet.solana.com`

##### 示例 `solana` 命令行配置

```bash
solana config set --url https://devnet.solana.com
```

##### 示例 `solana-validator` 命令行

```bash
$ solana-validator \
    --identity ~/validator-keypair.json \
    --vote-account ~/vote-account-keypair.json \
    --trusted-validator dv1LfzJvDF7S1fBKpFgKoKXK5yoSosmkAdfbxBo1GqJ \
    --no-untrusted-rpc \
    --ledger ~/validator-ledger \
    --rpc-port 8899 \
    --dynamic-port-range 8000-8010 \
    --entrypoint entrypoint.devnet.solana.com:8001 \
    --expected-genesis-hash EtWTRABZaYq6iMfeYKouRu166VU2xqa1wcaWoxPkrZBG \
    --wal-recovery-mode skip_any_corrupted_record \
    --limit-ledger-size
```

`--trusted-validator`由 Solana 运行

## Testnet（测试网）

- Testnet是我们在实时群集上重点测试最新发布功能的地方，尤其侧重于网络性能，稳定性和验证程序行为。
- 集群[Tour de SOL](tour-de-sol.md)计划在Testnet上运行，在该计划中，我们接受恶意行为和对网络的攻击，以帮助我们发现和消除错误或网络漏洞。
- Testnet代币**不是真实的**
- Testnet可能会重置账本。
- Testnet包括用于空投的代币水龙头，用于应用程序测试
- Testnet通常运行比Devnet和Mainnet Beta都更新的软件版本
- 测试网 Gossip 入口： `entrypoint.testnet.solana.com:8001`
- Testnet的指标环境变量：
```bash
export SOLANA_METRICS_CONFIG="host=https://metrics.solana.com:8086,db=tds,u=testnet_write,p=c4fa841aa918bf8274e3e2a44d77568d9861b3ea"
```
- Testnet 的 RPC URL: `https://testnet.solana.com`

##### 示例 `solana` 命令行配置

```bash
solana config set --url https://testnet.solana.com
```

##### 示例 `solana-validator` 命令行

```bash
$ solana-validator \
    --identity ~/validator-keypair.json \
    --vote-account ~/vote-account-keypair.json \
    --trusted-validator 5D1fNXzvv5NjV1ysLjirC4WY92RNsVH18vjmcszZd8on \
    --trusted-validator ta1Uvfb7W5BRPrdGnhP9RmeCGKzBySGM1hTE4rBRy6T \
    --trusted-validator Ft5fbkqNa76vnsjYNwjDZUXoTWpP7VYm3mtsaQckQADN \
    --trusted-validator 9QxCLckBiJc783jnMvXZubK4wH86Eqqvashtrwvcsgkv \
    --no-untrusted-rpc \
    --ledger ~/validator-ledger \
    --rpc-port 8899 \
    --dynamic-port-range 8000-8010 \
    --entrypoint entrypoint.testnet.solana.com:8001 \
    --expected-genesis-hash 4uhcVJyU9pJkvQyS88uRDiswHXSCkY3zQawwpjk2NsNY \
    --wal-recovery-mode skip_any_corrupted_record \
    --limit-ledger-size
```

`--trusted-validator` 的身份是：

- `5D1fNXzv5NjV1ysLjirC4WY92RNsVH18vjmcszZd8on` - testnet.solana.com (Solana)
- `ta1Uvfb7W5BRPrdGnhP9RmeCKzBySGM1hTE4rBRy6T` - Break RPC 节点 (Solana)
- `Ft5fbkqNa76vnsjYNwjDZUXoTWpP7VYm3mtsaQckQADN` - Certus One
- `9QxCLckBiJc783jnMvXZubK4wH86Eqqvashtrwvcsgkv` - Algo|Stake

## Mainnet Beta（主网 Beta）

适用于早期代币持有者和启动合作伙伴，未经许可的持久集群。 当前，奖励和通货膨胀被禁用。

- 在Mainnet Beta上发行的代币是**真实的**SOL
- 如果您通过我们的硬币清单拍卖等方式支付了购买/发行代币的费用，则这些代币将在Mainnet Beta上转移。
  - 注意：如果您使用的是非命令行钱包，例如集群[Solflare](wallet-guide/solflare.md)，则该钱包将始终连接到Mainnet Beta。
- Mainnet Beta 的 Gossip 入口： `entrypoint.mainnet-beta.solana.com:8001`
- Mainnet Beta的指标环境变量：
```bash
export SOLANA_METRICS_CONFIG="host=https://metrics.solana.com:8086,db=mainnet-beta,u=mainnet-beta_write,p=password"
```
- Mainnet Beta 的 RPC URL： `https://api.mainnet-beta.solana.com`

##### 示例 `solana` 命令行配置

```bash
solana config set --url https://api.mainnet-beta.solana.com
```

##### 示例 `solana-validator` 命令行

```bash
$ solana-validator \
    --identity ~/validator-keypair.json \
    --vote-account ~/vote-account-keypair.json \
    --trusted-validator 7Np41oeYqPefeNQEHSv1UDhYrehxin3NStELsSKCT4K2 \
    --trusted-validator GdnSyH3YtwcxFvQrVVJMm1JhTS4QVX7MFsX56uJLUfiZ \
    --trusted-validator DE1bawNcRJB9rVm3buyMVfr8mBEoyyu73NBovf2oXJsJ \
    --trusted-validator CakcnaRDHka2gXyfbEd2d3xsvkJkqsLw2akB3zsN1D2S \
    --no-untrusted-rpc \
    --ledger ~/validator-ledger \
    --rpc-port 8899 \
    --private-rpc \
    --dynamic-port-range 8000-8010 \
    --entrypoint entrypoint.mainnet-beta.solana.com:8001 \
    --entrypoint entrypoint2.mainnet-beta.solana.com:8001 \
    --entrypoint entrypoint3.mainnet-beta.solana.com:8001 \
    --entrypoint entrypoint4.mainnet-beta.solana.com:8001 \
    --entrypoint entrypoint5.mainnet-beta.solana.com:8001 \
    --expected-genesis-hash 5eykt4UsFv8P8NJdTREpY1vzqKqZKvdpKuc147dw2N9d \
    --wal-recovery-mode skip_any_corrupted_record \
    --limit-ledger-size
```

所有的四个 `--trusted-validator（可信验证节点）` 由 Solana 运行
