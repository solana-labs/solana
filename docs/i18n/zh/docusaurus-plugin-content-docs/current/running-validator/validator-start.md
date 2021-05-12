---
title: 启动验证程序
---

## 配置 Solana CLI

Solana cli包含`get`和`set`配置命令，可自动为cli命令设置`--url`参数。 例如：

```bash
solana config set --url http://devnet.solana.com
```

尽管本节演示了如何连接到Devnet群集，但其他的[Solana群集](../clusters.md)步骤与此类似。

## 确认集群可以访问

在附加验证节点之前，通过获取事务计数来明确检查集群是否可被您的机器访问：

```bash
solana 交易数统计
```

查看 [性能展板](https://metrics.solana.com:3000/d/monitor/cluster-telemetry) 来了解集群活动的细节。

## 确认您的安装程序

尝试运行以下命令以加入八卦网络并查看集群中的所有其他节点：

```bash
solana-gossip spy --entrypoint devnet.solana.com/8001
# 按^C 退出
```

## 启用 CUDA

如果您的机器安装了 CUDA 的 GPU \(Linux-only currently\)，请将 `--cuda` 参数包含到 `solana-validator`。

当您的验证程序启动后，请查找以下日志消息来确认CUDA已启用： `"[<timestamp> solana::validator] CUDA is enabled"`

## 系统调试

### Linux 系统
#### 自动模式
Solana代码库有一个守护程序，用于调整系统设置以优化性能(即通过增加OS UDP缓冲区和文件映射限制)。

守护进程(`solana-sys-tuner`) 已包含在solana二进制版本中。 在每次软件升级之后，在重新启动验证节点*之前*进行重新启动，以确保配置了系统建议的最新设置。 要运行它：

运行：

```bash
sudo solana-sys-tuner --user $(whoami) > sys-tuner.log 2>&1 &
```

#### 手动模式
如果希望自己管理系统设置，您可以使用以下命令。

##### **增加UDP缓冲区**
```bash
sudo bash -c "cat >/etc/sysctl.d/20-solana-udp-buffers.conf <<EOF
# 增加UDP缓存大小
net.core.rmem_default = 134217728
net.core.rmem_max = 134217728
net.core.wmem_default = 134217728
net.core.wmem_max = 134217728
EOF"
```
```bash
sudo sysctl -p /etc/sysctl.d/20-solana-udp-buffers.conf
```

##### **增加内存映射文件限制**
```bash
sudo bash -c "cat >/etc/sysctl.d/20-solana-mmaps.conf <<EOF
# 增加内存映射文件限制
vm.max_map_count = 500 000
EOF"
```
```bash
sudo sysctl -p /etc/sysctl.d/20-solana-mmaps.conf
```
新增内容
```
LimitNOFILE=500000
```
到系统服务文件的 `[Service]`（如果您有使用的话），否则请以其他方式添加
```
DefaultLimitNOFILE=500000
```
到 `[Manager]` `/etc/systemd/system.conf`。
```bash
sudo systemctl daemon-reload
```
```bash
sudo bash -c "cat >/etc/security/limits.d/90-solana-nofiles.conf <<EOF
# 增加流程文件描述器的计数上限
* - nofile 500 000
EOF"
```
```bash
### 关闭所有打开的会话 (然后再注销) ###
```

## 生成身份信息

通过运行以下操作为您的验证节点创建身份密钥：

```bash
solana-keygen new -o ~/validator-keypair.json
```

现在可以通过运行以下操作查看身份公钥：

```bash
solana-keygen pubkey ~/validator-keypair.json
```

> 注意："validator-keypair.json"文件也是您的 \(ed25519\) 私钥。

### 纸钱包身份

您可以为身份文件创建一个纸钱包，而不用将密钥对文件写入到磁盘：

```bash
solana-keygen new --no-outfile
```

现在可以通过运行以下操作查看相应的身份公钥：

```bash
solana-keygen pubkey ASK
```

然后输入您的种子短语。

查看 [纸钱包使用](../wallet-guide/paper-wallet.md) 获取更多信息。

---

### 虚拟密钥

您可以使用solana-keygen生成一个自定义的虚拟密钥。 例如：

```bash
solana-keygen grind --starts-with e1v1s:1
```

根据请求的字符串，可能需要几天时间才能匹配...

---

您的验证节点身份密钥独特识别了您在网络中的验证节点。 **备份此信息至关重要。**

如果您不备份此信息，那么如果您无法访问验证节点的话，将无法对其进行恢复。 如果发生这种情况，您将失去SOL TOO的奖励。

要备份您的验证节点识别密钥， **请备份您的"validator-keypair.json" 文件或种子短语到一个安全位置。**

## 更多 Solana CLI 配置

现在您有了密钥对，将solana配置设置为对以下所有命令使用验证节点密钥对：

```bash
solana config set --keypair ~/validator-keypair.json
```

您应该看到以下输出：

```text
Wallet Config Updated: /home/solana/.config/solana/wallet/config.yml
* url: http://devnet.solana.com
* keypair: /home/solana/validator-keypair.json
```

## 空投 & 检查验证节点账户余额

空投自己一些SOL即可开始使用：

```bash
solana airdrop 10
```

请注意，空投只能在Devnet和Testnet上使用。 每次请求都限制在 10 个 SOL。

要查看您当前的余额：

```text
solana balance
```

或查看更详细的信息：

```text
solana balance --lamports
```

在这里阅读更多关于 [SOL与lamports 之间的差异](../introduction.md#what-are-sols)。

## 创建一个投票账户

如果您还没有进行这一步，请创建一个投票帐户密钥对并在网络上创建该投票帐户。 如果完成了此步骤，则应该在Solana运行时目录中看到“ vote-account-keypair.json”：

```bash
solana-keygen new -o ~/vote-account-keypair.json
```

以下命令可用于使用所有在区块链上创建投票帐户的默认选项：

```bash
solana create-vote-account ~/vote-account-keypair.json ~/validator-keypair.json
```

阅读更多关于 [创建和管理一个投票账户](vote-accounts.md)的信息。

## 可信的验证程序

如果您知道并信任其他验证节点节点，则可以在命令行中使用`solana-validator`的参数`--trusted-validator<PUBKEY>`来指定。 您可以通过重复参数`--trusted-validator<PUBKEY1> --trusted-validator <PUBKEY2>`来指定多个。 这有两种作用，一种是当验证节点使用`--no-untrusted-rpc`引导时，它只会询问那组受信任的节点来下载创世区块和快照数据。 另一个是结合`--halt-on-trusted-validator-hash-mismatch`选项，它将监视八卦上其他受信任节点的整个帐户状态的merkle根哈希，如果哈希有任何不匹配，验证节点将停止该节点，以防止验证节点投票或处理可能不正确的状态值。 目前，验证节点在其上发布哈希的插槽已与快照间隔绑定。 为了使该功能生效，应将可信集中的所有验证节点设置为相同的快照间隔值或相同的倍数。

我们强烈建议您使用这些选项来防止恶意快照状态下载或帐户状态差异。

## 连接您的验证节点

通过运行以下命令连接到集群：

```bash
solana-validator \
  --identity ~/validator-keypair.json \
  --vote-account ~/vote-account-keypair.json \
  --ledger ~/validator-ledger \
  --rpc-port 8899 \
  --entrypoint devnet.solana.com:8001 \
  --limit-ledger-size \
  --log ~/solana-validator.log
```

要强制验证日志记录到控制台，请添加 `--log -` 参数，否则验证程序将自动登录到一个文件。

> 注意：您可以使用 [纸钱包种子短语](../wallet-guide/paper-wallet.md) 用于您的 `--identity` 和/或 `--authorized-panitor` 密钥对。 要使用这些参数，请将各自的参数作为 `solana-validator --idential ASK ... --authorized-lister ASK ...` 并且您将会收到 输入您的种子短语和可选密码的提示。

通过打开一个新终端并运行以下命令来确认连接到网络的验证节点：

```bash
solana-gossip spy --entrypoint devnet.solana.com:8001
```

如果您的验证节点已连接，其公钥和IP地址将出现在列表中。

### 控制本地网络端口分配

默认情况下，验证节点将在8000-1000范围内动态选择可用的网络端口，可能会覆盖 `--dynamic-port-range`。 例如： `solana-validator --dynamic-port-range 11000-110...` 将限制验证节点到 11000-11010 端口。

### 限制账本大小以节省磁盘空间
`--limit-ledger-size` 参数允许您指定磁盘保留多少个账本[碎片](../terminology.md#shred)。 如果您没有配置该参数，验证节点将保留整个账本直到磁盘空间满了为止。

保持账本磁盘使用量的默认值小于 500GB。  如果需要，可以通过添加参数到 `--limit-ledger-size` 来增加或减少磁盘的使用。 查看 `solana-validator --help` 来配置 `--limit-ledger-size` 所使用的默认限制值。  关于选择一个普通限制值的更多信息请参看 [这里](https://github.com/solana-labs/solana/blob/583cec922b6107e0f85c7e14cb5e642bc7dfb340/core/src/ledger_cleanup_service.rs#L15-L26)。

### 系统单位
将验证程序作为系统单元运行是管理后台运行的一种简单方法。

假定您的机器上有一个名为 `sol` 的用户，通过以下命令来创建 `/etc/systemd/system/sol.service` 文件：
```
[Unit]
Description=Solana Validator
After=network.target
Wants=solana-sys-tuner.service
StartLimitIntervalSec=0

[Service]
Type=simple
Restart=always
RestartSec=1
User=sol
LimitNOFILE=500000
LogRateLimitIntervalSec=0
Environment="PATH=/bin:/usr/bin:/home/sol/.local/share/solana/install/active_release/bin"
ExecStart=/home/sol/bin/validator.sh

[Install]
WantedBy=multi-user.target
```

现在创建 `/home/sol/bin/validator.sh` 来包含 `solana-validator` 所需的命令行。  确保运行 `/home/sol/bin/validator.sh` 来手动启动验证程序。 别忘了将其标记为 `chmod +x /home/sol/bin/validator.sh`

开启服务：
```bash
$ sudo systemctl enable --now sol
```

### 日志
#### 日志输出调整

验证节点导出到日志的消息可以由 `RUST_LOG` 环境变量控制。 在 [文档](https://docs.rs/env_logger/latest/env_logger/#enabling-logging)中也可以找到详细的 Rust crate `env_logger` 信息。

请注意，如果减少日志记录输出，则可能难以调试以后遇到的问题。 如果需要团队的支持，则任何变更都必须恢复，并且在提供帮助之前应重现问题。

#### 日志切换

由 `--log ~/solana-validator.log`指定的验证器日志文件会随着时间的推移变得很大，因此建议配置日志切换。

验证节点在收到`USR1`信号时将重新打开其信号，该信号是启用日志切换的基本原语。

#### 使用日志切换

`logrotate`的一个示例设置中，它假定验证节点作为名为`sol.service`的系统服务运行，并在/home/sol/solana-validator.log中写入日志文件：
```bash
# 设置日志切换

cat > logrotate.sol <<EOF
/home/sol/solana-validator.log {
  rotate 7
  daily
  missingok
  postrotate
    systemctl kill -s USR1 sol.service
  endscript
}
EOF
sudo cp logrotate.sol /etc/logrotate.d/sol
systemctl restart logrotate.service
```

### 禁用端口检查以加快重启速度
验证节点正常运行后，您可以通过在`solana-validator`命令行中添加`--no-port-check`标志来减少重新启动验证节点所需的时间。

### 禁用快照压缩以减少CPU使用率
如果不将快照提供给其他验证节点，则可以禁用快照压缩功能以减少CPU负载，但这样会花费更多的本地快照存储磁盘使用量。

在`solana-validator`命令行参数中添加`--snapshot-compression none`参数，然后重新启动验证节点。

### 使用具有溢出功能的ramdisk交换帐户数据库以减少SSD磨损
如果您的机器有大量的RAM，可以使用tmpfs ramdisk ([tmpfs](https://man7.org/linux/man-pages/man5/tmpfs.5.html)) 来保持账户数据库

使用tmpfs时，还必须在计算机上配置swap，以避免定期用完tmpfs空间。

建议使用300GB的tmpfs分区，并附带250GB的交换分区。

示例配置：
1. `sudo mkdir /mnt/solana-accounts`
2. 添加一个300GB tmpfs paragraph ，添加一个包含 `tmpfs/mnt/solana-account tmpfs rw,size=300G, ser=sol 0` 的新行到 `/etc/fstab`(假设您的验证节点运行在用户"sol"下)。  **请注意：如果错误地编辑了/etc/fstab，那么您的机器可能无法启动**
3. 创建至少 250GB 交换空间
  - 在本说明的其余部分中，选择要代替`SWAPDEV`的设备。 理想情况下，在快速磁盘上选择250GB或更大的可用磁盘分区。 如果全部都不可用，则通过`sudo dd if=/dev/zero of=/swapfile bs=1MiB count=250KiB`创建一个交换文件，将其权限设置为 `sudo chmod 0600 /swapfile` 并使用 `/swapfile` 作为 `SWAPDEV` 这些说明的其余部分。
  - 使用 `sudo mkswap SWAPDEV `将设备格式化为交换设备
4. 一个包含 `SWAPDEV 交换器默认值 0`的新行，将 `/etc/fstab` 添加到交换文件
5. 启用与 `sudo swapon -a` 的交换并以 `sudo mount /mnt/solana-accounts/` 挂载tmps
6. 确认交换正在使用 `free -g` 并且tmpfs 被挂载 `mount`

现在将 `--accounts /mnt/solana-account` 参数添加到您的 `solana-validator`命令行参数并重启验证节点。

### 账户索引

随着集群中账户数量的增加，该核算群组的核算数量也在增加， 账户数据 RPC请求扫描整个账户集的时候 -- 例如 [`getProgramAccounts`](developing/clients/jsonrpc-api.md#getprogramaccounts) 和 [SPL-token-specific requests](developing/clients/jsonrpc-api.md#gettokenaccountsbydelegate) -- 效果可能比较差。 如果您的验证节点需要支持这些请求中的任何一个，则可以使用`--account-index`参数来激活一个或多个内存帐户索引，该索引通过按关键字段为帐户建立索引来显着提高RPC性能。 当前支持以下参数值：

- `program-id`: 每个帐户都由其拥有的程序索引; 通过 [`getProgramAccounts`](developing/clients/jsonrpc-api.md#getprogramaccounts)执行
- `spl-token-mint`: 每个SPL 代币帐户由其代币铸造索引; 通过 [getTokenAccountsByDelegate](developing/clients/jsonrpc-api.md#gettokenaccountsbydelegate)和 [getTokenLargestAccounts](developing/clients/jsonrpc-api.md#gettokenlargestaccounts) 来使用
- `spl-token-owner`: 每一个 SPL token 帐户按 token-owner 地址索引； 由 [getTokenAccountsByOwner](developing/clients/jsonrpc-api.md#gettokenaccountsbyowner)和 [`getProgramAccounts`](developing/clients/jsonrpc-api.md#getprogramaccounts) 请求包含一个spl-token-owners的过滤器。
