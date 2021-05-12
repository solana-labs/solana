---
title: 添加 Solana 到您的交易所
---

本指南描述了如何将 Solana 的原生代币 SOL 添加到某个加密货币交易所。

## 节点设置

我们强烈建议在高级计算机/云端设置至少两个节点实例， 立即升级到较新的版本，并随时注意自带的监测工具的服务操作。

这样设置可以让您：
- 为 Solana mainnet-beta 集群设置一个可信的网关来获取数据和提交取现交易
- 完全控制保留历史区块数据的多少
- 即使某个节点失败仍然保持您的服务可用性

Solana 节点需要较高的计算力来处理我们的快速区块和高 TPS 。  关于具体要求，请参阅[硬件建议](../running-validator/validator-reqs.md)。

运行一个 api 节点：

1. [安装 Solana 命令行工具](../cli/install-solana-cli-tools.md)
2. 启动验证节点时至少使用以下参数：

```bash
solana-validator \
  --ledger <LEDGER_PATH> \
  --entrypoint <CLUSTER_ENTRYPOINT> \
  --expected-genesis-hash <EXPECTED_GENESIS_HASH> \
  --rpc-port 8899 \
  --no-voting \
  --enable-rpc-transaction-history \
  --limit-ledger-size \
  --trusted-validator <VALIDATOR_ADDRESS> \
  --no-untrusted-rpc
```

自定义 `--ledger` 到您所需的账本存储位置， `--rpc-port` 到您想要显示的端口。

`--entrypoint` and `--experted-genesis-hash` 参数都针对您正在加入的集群。 [主网 Beta 的当前参数](../clusters.md#example-solana-validator-command-line-2)

`--limit-ledger-size` 参数允许您指定保留节点的多少个账本 [shreds](../terminology.md#shred) 在磁盘上。 如果您没有配置该参数，验证节点将保留整个账本直到磁盘空间满了为止。  保持账本磁盘使用量的默认值小于 500GB。  如果需要，可以通过添加参数到 `--limit-ledger-size` 来增加或减少磁盘的使用。 查看 `solana-validator --help` 来配置 `--limit-ledger-size` 所使用的默认限制值。  关于选择一个普通限制值的更多信息请参看 [这里](https://github.com/solana-labs/solana/blob/583cec922b6107e0f85c7e14cb5e642bc7dfb340/core/src/ledger_cleanup_service.rs#L15-L26).

指定一个或多个 `--trusted-validator` 参数可以保护您免遭恶意快照的攻击。 [更多关于使用可信验证程序启动的值](../running-validator/validator-start.md#trusted-validators)

可选参数：

- `--private-rpc` 防止您的 RPC 端口被其他节点发布
- `--rpc-bind-address` 允许您指定一个不同的 IP 地址绑定 RPC 端口

### 自动重启和监测

我们建议将每个节点配置退出时自动重启，以确保尽可能少地丢失数据。 把 Solana 软件运行为一个系统服务是很好的选择。

对于监控，我们提供[`solana-watchtower`](https://github.com/solana-labs/solana/blob/master/watchtower/README.md)，它可以监视您的验证节点，并且通过 `solana-validator` 检测节点是否不健康。 它可以直接配置 Slack、Telegram 、Discord 或 Twillio 来提醒您。 详情请运行 `solana-watchtower --help`。

```bash
solana-watchtower --validator-identity <YOUR VALIDATOR IDENTITY>
```

#### 新软件发布公告

我们经常发布新软件(大约每周一版)。 有时较新的版本包含不兼容的协议调整，这时候需要及时更新软件，以避免出块产生的错误。

我们发布的所有类型的官方公告(正常的和安全)都是通过一个叫做[`#mb-annound`](https://discord.com/channels/428295358100013066/669406841830244375) (`mb` 表示 `mainnet-beta`)。

就像已质押的验证节点，我们期望任何交易所操作的验证节点在正常版本后的一个或两个工作日内尽早更新。 对于安全相关的信息，可能会采取更紧急的行动。

### 账本持续性

默认情况下，您的每个节点都通过可信验证节点提供的快照启动。 这个快照反映了区块链当前的状态，但不包含完整的历史帐本。 如果您的一个节点退出并且通过新的快照启动，那么该节点上的账本中可能会出现一段缺失。 为了防止该问题， 将 `--no-snapshot-fetch` 参数添加到您的 `solana-validator` 命令，来接收历史账本数据（而不是快照）。

不要在初次启动时通过 `--no-snapshot-fetch` 参数，因为它不可能追溯到创世区块去启动节点。  相反，您需要先启动快照，然后添加 `--no-snapshot-quetch` 参数来重启。

重要的一点是需要注意，在任何时候您的节点从网络其他地方可获取的可用历史账本数量都是有限的。  一旦运行，如果验证节点经历了重大故障，它们可能无法跟上网络，需要从可信的验证节点下载新的快照。  这样做的时候，您的验证节点在它的历史账本数据中将出现一个无法填补的空白。


### 最小化验证节点端口风险

验证节点要求从所有其他的 Solana 验证程序中打开 UDP 和 TCP 端口传入流量。   虽然这是最有效率的操作模式，我们也强烈推荐，但是可以将验证节点限制为只需要从另外一个 Solana 验证节点流量接入。

首先添加 `--restricted-reparir-only-mode` 参数。  这将会让验证节点在受限制的模式下运行，它将不会收到其他验证节点的消息，而是要不断联系其他验证节点获取区块。  验证节点只能使用 *Gossip* 和 *ServeR* ("服务修理") 端口传输 UDP 包到其他验证节点，并且只有在其 *Gossip* 和 *Repair* 端口上接收 UDP 包。

*Gossip* 端口是双向的，允许您的验证节点保持与其他集群的联系。  因为Turbine 现在已被禁用，因此您的验证节点需要在 *ServerR* 上传输信息，以便提出修理请求，从网络其余部分获取新区块。  然后您的验证节点将收到其他验证节点在 *Repair* 端口上的维修回应。

要进一步限制验证节点只从一个或多个验证器请求区块，您首先确定该验证节点身份的 Pubkey 为每一个 PUBKEY 添加 `--gossip-pull-validator PUBKEY --resurir-validator PUBKEY` 参数。  这将使你的验证节点成为您添加的每个验证节点上的资源流量， 您是可以这样操作的，并且只有在与目标验证节点请求后才能进行。

现在您的验证节点只能与特别指出的验证节点通信并且只能在 *Gossip*，*Repair* 和 *ServeR* 端口上通信。

## 设置存款账户

Solana 帐户不需要任何链上的初始化设置；只要有 SOL 余额，它们就自动出现。 您可以使用任何我们的 [钱包工具](../wallet-guide/cli.md) 生成一个 Solana 密钥，来设置一个交易所存款帐户。

我们建议您为每个用户配置一个独特的存款帐户。

Solana 帐户在每个 epoch 都收取一次 [ 租金 ](developing/programming-model/accounts.md#rent)，但如果它们的 SOL 价值包括两年，就可以免除租金。 想要找到您存款账户的最低免租余额，请查询[`getMinimumBalanceForRentExemption` 端点](developing/clients/jsonrpc-api.md#getminimumbalanceforrentexemption)：

```bash
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc": "2.0","id":1,"method":"getMinimumBalanceForRentExemption","params":[0]}' localhost:8899

{"jsonrpc":"2.0","result":890880,"id":1}
```

### 离线账户

为了提高的安全性，您可能想离线保存一个或多个收藏账户的密钥。 这时候您需要使用我们的 [ 离线方法](../offline-signing.md) 将 SOL 转移到热钱包。

## 正在等待充值

如果某个用户想 SOL 存入您的交易所，请指示他们发送一笔金额到相应的存款地址。

### 区块投票

您可以使用 Solana API 节点的 JSON-RPC 服务来跟踪交易所的所有存款帐户，对每个确认的区块进行调查或检查感兴趣的地址。

- 要确定哪些区块处于可用状态，请发送 [`getConfirmedBlocks` request](developing/clients/jsonrpc-api.md#getconfirmedblocks)，通过您已经处理过的最后一个块作为启动槽参数：

```bash
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc": "2.0","id":1,"method":"getConfirmedBlocks","params":[5]}' localhost:8899

{"jsonrpc":"2.0","result":[5,6,8,9,11],"id":1}
```

不是每个 Slot 都会出块，所以在整数序列中可能存在缺口。

- 对于每个块，可以通过 [`getConfirmedBlock` request](developing/clients/jsonrpc-api.md#getconfirmedblock) 请求其包含的内容:

```bash
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc": "2.0","id":1,"method":"getConfirmedBlock","params":[5, "json"]}' localhost:8899

{
  "jsonrpc": "2.0",
  "result": {
    "blockhash": "2WcrsKSVANoe6xQHKtCcqNdUpCQPQ3vb6QTgi1dcE2oL",
    "parentSlot": 4,
    "previousBlockhash": "7ZDoGW83nXgP14vnn9XhGSaGjbuLdLWkQAoUQ7pg6qDZ",
    "rewards": [],
    "transactions": [
      {
        "meta": {
          "err": null,
          "fee": 5000,
          "postBalances": [
            2033973061360,
            218099990000,
            42000000003
          ],
          "preBalances": [
            2044973066360,
            207099990000,
            42000000003
          ],
          "status": {
            "Ok": null
          }
        },
        "transaction": {
          "message": {
            "accountKeys": [
              "Bbqg1M4YVVfbhEzwA9SpC9FhsaG83YMTYoR4a8oTDLX",
              "47Sbuv6jL7CViK9F2NMW51aQGhfdpUu7WNvKyH645Rfi",
              "11111111111111111111111111111111"
            ],
            "header": {
              "numReadonlySignedAccounts": 0,
              "numReadonlyUnsignedAccounts": 1,
              "numRequiredSignatures": 1
            },
            "instructions": [
              {
                "accounts": [
                  0,
                  1
                ],
                "data": "3Bxs3zyH82bhpB8j",
                "programIdIndex": 2
              }
            ],
            "recentBlockhash": "7GytRgrWXncJWKhzovVoP9kjfLwoiuDb3cWjpXGnmxWh"
          },
          "signatures": [
            "dhjhJp2V2ybQGVfELWM1aZy98guVVsxRCB5KhNiXFjCBMK5KEyzV8smhkVvs3xwkAug31KnpzJpiNPtcD5bG1t6"
          ]
        }
      }
    ]
  },
  "id": 1
}
```

` 原先余额 ` 和 ` 交易后余额 ` 字段能让您跟踪余额每个账户中的变动，而无需解析整个交易。 他们将每个账户的最初和交易后余额分别列出在 [ lamports ](../terminology.md#lamport) 中，并索引到 `账户` 列表。 例如，您准备充值的地址是 ` 47Sbuv6jL7CViK9F2NMW51aQGhfdpUu7WNvKyH645Rfi `，它表示一笔 218099990000 - 207099990000 = 11000000000 lamports = 11 SOL 的交易。

如果需要更多关于交易类型或其他细节的信息，您可以用二进制格式从 RPC 请求区块，然后使用 [Rust SDK](https://github.com/solana-labs/solana) 或 [Javascript SDK](https://github.com/solana-labs/solana-web3.js) 进行解析。

### 地址历史

您也可以查询特定地址的交易历史记录。 这通常 *不是* 一种追踪您所有插槽的所有存款地址的可行方法， 但可能检查一段时间内的几个账户非常有用。

- 向 api 节点发送 [`getConfirmedSignaturesFors2`](developing/clients/jsonrpc-api.md#getconfirmedsignaturesforaddress2) 请求：

```bash
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc": "2.0","id":1,"method":"getConfirmedSignaturesForAddress2","params":["6H94zdiaYfRfPfKjYLjyr2VFBg6JHXygy84r3qhc3NsC", {"limit": 3}]}' localhost:8899

{
  "jsonrpc": "2.0",
  "result": [
    {
      "err": null,
      "memo": null,
      "signature": "35YGay1Lwjwgxe9zaH6APSHbt9gYQUCtBWTNL3aVwVGn9xTFw2fgds7qK5AL29mP63A9j3rh8KpN1TgSR62XCaby",
      "slot": 114
    },
    {
      "err": null,
      "memo": null,
      "signature": "4bJdGN8Tt2kLWZ3Fa1dpwPSEkXWWTSszPSf1rRVsCwNjxbbUdwTeiWtmi8soA26YmwnKD4aAxNp8ci1Gjpdv4gsr",
      "slot": 112
    },
    {
      "err": null,
      "memo": null,
      "signature": "dhjhJp2V2ybQGVfELWM1aZy98guVVsxRCB5KhNiXFjCBMK5KEyzV8smhkVvs3xwkAug31KnpzJpiNPtcD5bG1t6",
      "slot": 108
    }
  ],
  "id": 1
}
```

- 对于返回的每个签名，发送 [`getConsulmedTransaction`](developing/clients/jsonrpc-api.md#getconfirmedtransaction) 请求来获取交易细节：

```bash
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc": "2.0","id":1,"method":"getConfirmedTransaction","params":["dhjhJp2V2ybQGVfELWM1aZy98guVVsxRCB5KhNiXFjCBMK5KEyzV8smhkVvs3xwkAug31KnpzJpiNPtcD5bG1t6", "json"]}' localhost:8899

// 结果
{
  "jsonrpc": "2.0",
  "result": {
    "slot": 5,
    "transaction": {
      "message": {
        "accountKeys": [
          "Bbqg1M4YVVfbhEzwA9SpC9FhsaG83YMTYoR4a8oTDLX",
          "47Sbuv6jL7CViK9F2NMW51aQGhfdpUu7WNvKyH645Rfi",
          "11111111111111111111111111111111"
        ],
        "header": {
          "numReadonlySignedAccounts": 0,
          "numReadonlyUnsignedAccounts": 1,
          "numRequiredSignatures": 1
        },
        "instructions": [
          {
            "accounts": [
              0,
              1
            ],
            "data": "3Bxs3zyH82bhpB8j",
            "programIdIndex": 2
          }
        ],
        "recentBlockhash": "7GytRgrWXncJWKhzovVoP9kjfLwoiuDb3cWjpXGnmxWh"
      },
      "signatures": [
        "dhjhJp2V2ybQGVfELWM1aZy98guVVsxRCB5KhNiXFjCBMK5KEyzV8smhkVvs3xwkAug31KnpzJpiNPtcD5bG1t6"
      ]
    },
    "meta": {
      "err": null,
      "fee": 5000,
      "postBalances": [
        2033973061360,
        218099990000,
        42000000003
      ],
      "preBalances": [
        2044973066360,
        207099990000,
        42000000003
      ],
      "status": {
        "Ok": null
      }
    }
  },
  "id": 1
}
```

## 发送提现请求

要满足用户的提款请求, 您必须生成一笔 Solana 转账交易，并将其发送到 API 节点来扩散到集群中。

### 同步

发送同步传输到 Solana 集群可以让您轻松保证转账的成功并由集群确定最终性。

Solana的命令行工具提供了一个用于生成、提交和确认转账交易的简单命令， `solana transfer`。 默认情况下，该方法将等待并跟踪 stderr 的进度，直到集群确认了某笔交易。 如果交易失败，它将报告任何类型的交易错误。

```bash
solana transfer <USER_ADDRESS> <AMOUNT> --keypair <KEYPAIR> --url http://localhost:8899
```

[Solana Javascript SDK](https://github.com/solana-labs/solana-web3.js) 为 JS 生态提供了类似的方法。 使用 `SystemProgram` 创造一笔转账交易，然后使用 `sendAndConfirmTransaction` 方法提交。

### 异步

为了更大的灵活性，您可以异步提交提现转账。 在这些情况下，您有责任验证交易的成功性并由集群最终确认。

** 请注意：** 每笔交易都包含一个 [ 最新区块哈希 ](developing/programming-model/transactions.md#blockhash-format) 表明它在线。 在某笔提现交易没有被集群确认或最终确定的时候，如果要重新提现，等待这个区块哈希过期是非常 **重要** 的。 否则，你将会面临双花的风险。 更多内容请参见下方 [blockhash expiration](#blockhash-expiration)。

首先，使用 [`getFees` 端点](developing/clients/jsonrpc-api.md#getfees) 或 CLI 命令获取最近的区块哈希：

```bash
solana fees --url http://localhost:8899
```

在命令行工具中，通过 `--no-wait` 参数发送异步传输，使用 `--blockhash` 参数包含您最近的区块哈希：

```bash
solana transfer <USER_ADDRESS> <AMOUNT> --no-wait --blockhash <RECENT_BLOCKHASH> --keypair <KEYPAIR> --url http://localhost:8899
```

您也可以手动化生成、签名和序列化一笔交易，然后用 JSON-RPC [`发送交易` 端点](developing/clients/jsonrpc-api.md#sendtransaction) 将它关闭到某个集群。

#### 交易确认 & 最终性

使用 [`getSignatureStatuses` JSON-RPC 端点](developing/clients/jsonrpc-api.md#getsignaturestatuses) 获取一批交易的状态。 `确认` 字段报告了自交易处理后，有多少 [已确认区块](../terminology.md#confirmed-block) 。 如果 `confirmations: null`，那么它就是 [已经确认](../terminology.md#finality)。

```bash
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0", "id":1, "method":"getSignatureStatuses", "params":[["5VERv8NMvzbJMEkV8xnrLkEaWRtSz9CosKDYjCJjBRnbJLgp8uirBgmQpjKhoR4tjF3ZpRzrFmBV6UjKdiSZkQUW", "5j7s6NiJS3JAkvgkoc18WVAsiSaci2pxB2A6ueCJP4tprA2TFg9wSyTLeYouxPBJEMzJinENTkpA52YStRW5Dia7"]]}' http://localhost:8899

{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 82
    },
    "value": [
      {
        "slot": 72,
        "confirmations": 10,
        "err": null,
        "status": {
          "Ok": null
        }
      },
      {
        "slot": 48,
        "confirmations": null,
        "err": null,
        "status": {
          "Ok": null
        }
      }
    ]
  },
  "id": 1
}
```

#### 区块哈希过期

当您使用 [`getFees` endpoint](developing/clients/jsonrpc-api.md#getfees) 或 `solana fees` 请求您提款交易最近的区块哈希，响应将包括 `lastValidSlot`，有效区块哈希的最后一个插槽。 您可以使用 [`getSlot` query](developing/clients/jsonrpc-api.md#getslot) 检查集群插槽；一旦集群槽大于`lastValidSlot`，那么使用该区块哈希的提现交易永远不会成功。

您也可以通过发送一个以区块哈希作为参数 [`getFeeCalculatorForBlockhash`](developing/clients/jsonrpc-api.md#getfeecalculatorforblockhash) 的请求，来再次确认某个区块哈希是否仍然有效。 如果响应值为空，那么该区块哈希已经过期，提现请求就一定不会成功。

### 验证用户提供的提款账户地址

由于提款是不可逆过程，因此最好在提款确认之前对用户提供的帐户地址进行验证，以防止用户资产意外丢失。

Solana 的普通账户地址是一个 256 位 ed25519 公钥的 Base58 编码字符串。 并非所有位图案都是 ed25519 曲线的有效公共密钥， 这样可以确保用户提供的帐户地址至少是正确的 ed25519 公钥。

#### Java

这是验证用户提供的地址为有效 ed25519 公钥的 Java 示例：

下面的代码例子假设你正在使用 Maven。

`pom.xml`:

```xml
<repositories>
  ...
  <repository>
    <id>spring</id>
    <url>https://repo.spring.io/libs-release/</url>
  </repository>
</repositories>

...

<dependencies>
  ...
  <dependency>
      <groupId>io.github.novacrypto</groupId>
      <artifactId>Base58</artifactId>
      <version>0.1.3</version>
  </dependency>
  <dependency>
      <groupId>cafe.cryptography</groupId>
      <artifactId>curve25519-elisabeth</artifactId>
      <version>0.1.0</version>
  </dependency>
<dependencies>
```

```java
import io.github.novacrypto.base58.Base58;
import cafe.cryptography.curve25519.CompressedEdwardsY;

public class PubkeyValidator
{
    public static boolean verifyPubkey(String userProvidedPubkey)
    {
        try {
            return _verifyPubkeyInternal(userProvidedPubkey);
        } catch (Exception e) {
            return false;
        }
    }

    public static boolean _verifyPubkeyInternal(String maybePubkey) throws Exception
    {
        byte[] bytes = Base58.base58Decode(maybePubkey);
        return !(new CompressedEdwardsY(bytes)).decompress().isSmallOrder();
    }
}
```

## 支持 SPL 代币标准

[SPL 代币](https://spl.solana.com/token) 是在 Solana 区块链上创建和交易包装/合成代币的标准。

SPL 代币的工作流程类似于原生 SOL 代币，但本节将讨论它们的几个不同之处。

### 代币铸造

每种 *类型* 的 SPL 代币都是由一个 *铸造* 账号所产生。  该帐户存储了代币功能的元数据，如供应量、小数点数和对铸造的多种权限。  每个 SPL Token 帐户引用与它铸造相关的字段，并且只能与该种类型的 SPL 代币交互。

### 安装 `spl-token` CLI 工具

使用 `spl-token` 命令行功能查询和修改 SPL Token 帐户。 本部分提供的示例取决于能否在本地系统安装。

`spl-token` 从 Rust [crates.io](https://crates.io/crates/spl-token) 中通过 Rust `cargo` 命令行功能衍生出来的。 最新版本的 `cargo` 可以在 [rustuprers](https://rustup.rs)，通过方便的工具安装在您的平台。 一旦 `cargo` 安装完毕， `spl-toke` 可以通过以下命令获得：

```
cargo install spl-token-cli
```

然后您可以检查已安装的版本进行验证

```
spl-token --version
```

输出结果应该类似于

```text
spl-token-cli 2.0.1
```

### 创建帐户

SPL 代币账户包含了本地系统程序账户所不具备的额外要求：

1. 在创建 SPL Token 帐户之前，必须先存入一定数量的代币。   代币帐户可以使用 `spl-token create-account` 命令显式创建， 或者 `spl-token transfer --fund-receiving ...` 命令隐式创建。
1. 在生效期间，SPL Token 帐户必须保持 [rent-exempt](developing/programming-model/accounts.md#rent-exemption) 状态，因此在创建帐户时需要存入少量的原生 SOL 代币。 对于 SPL Token v2 账户，该数量为 0.00203928 SOL(2 039 280 lamports)。

#### 命令行
创建具有以下属性的 SPL 代币帐户：
1. 关联指定的铸造
1. 由资产账户的密钥所拥有

```
spl-token create-account <TOKEN_MINT_ADDRESS>
```

#### 示例：
```
$ spl-token create-account AkUFCWTXb3w9nY2n6SFJvBV6VwvFUCe4KBMCcgLsa2ir
Creating account 6VzWGL51jLebvnDifvcuEDec17sK6Wupi4gYhm5RzfkV
Signature: 4JsqZEPra2eDTHtHpB4FMWSfk3UgcCVmkKkP7zESZeMrKmFFkDkNd91pKP3vPVVZZPiu5XxyJwS73Vi5WsZL88D7
```

或者创建指定密钥对的 SPL 代币账户：
```
$ solana-keygen new -o token-account.json
$ spl-token create-account AkUFCWTXb3w9nY2n6SFJvBV6VwvFUCe4KBMCcgLsa2ir token-account.json
Creating account 6VzWGL51jLebvnDifvcuEDec17sK6Wupi4gYhm5RzfkV
Signature: 4JsqZEPra2eDTHtHpB4FMWSfk3UgcCVmkKkP7zESZeMrKmFFkDkNd91pKP3vPVVZZPiu5XxyJwS73Vi5WsZL88D7
```

### 检查账户余额

#### 命令行
```
spl-token balance <TOKEN_ACCOUNT_ADDRESS>
```

#### 示例：
```
$ solana balance 6VzWGL51jLebvnDifvcuEDec17sK6Wupi4gYhm5RzfkV
0
```

### 代币转移

发送代币的源账户是包含余额的实际代币账户。

但是收款人地址可以是一个普通的钱包帐户。  如果给定钱包关联的代币帐户不存在，那么将在发送交易的时候创建一个地址，条件是 `--fund-receiver` 所提供的参数。

#### 命令行
```
spl-token transfer <SENDER_ACCOUNT_ADDRESS> <AMOUNT> <RECIPIENT_WALLET_ADDRESS> --fund-recipient
```

#### 示例：
```
$ spl-token transfer 6B199xxzw3PkAm25hGJpjj3Wj3WNYNHzDAnt1tEqg5BN 1 6VzWGL51jLebvnDifvcuEDec17sK6Wupi4gYhm5RzfkV
发送 1 个代币
  发送方：6B199xxzw3PkAm25hGJpjj3Wj3WNYNHzDAnt1tEqg5BN
  接收方：6VzWGL51jLebvnDifvcuEDec17sK6Wupi4gYhm5RzfkV
签名：3R6tsog17QM8KfzbcbdP4aoMfwgo6hBggJDVy7dZPVmH2xbCWjEj31JKD53NzMrf25ChFjY7Uv2dfCDq4mGFFyAj
```

### 充值
因为每个 `(user, mint)` 对需要在链上有一个单独的帐户，所以建议交易所提前创建批量代币帐户，并分配给各个用户。 这些账户都由交易所账号密钥所拥有。

存款交易的监控应遵循上面描述的 [block polling](#poll-for-blocks) 方法。 每个新区块应该扫描获得铸造 SPL 代币的成功交易 [Transfer](https://github.com/solana-labs/solana-program-library/blob/096d3d4da51a8f63db5160b126ebc56b26346fc8/token/program/src/instruction.rs#L92) 或 [Transfer2](https://github.com/solana-labs/solana-program-library/blob/096d3d4da51a8f63db5160b126ebc56b26346fc8/token/program/src/instruction.rs#L252) 指令来引用用户帐户，然后查询 [代币账户余额](developing/clients/jsonrpc-api.md#gettokenaccountbalance) 更新。

[Considerations](https://github.com/solana-labs/solana/issues/12318) 正在扩展 `preBalance`和`postBalance` 交易状态元数据字段，来把 SPL代币余额转移包括进去。

### 提现
用户提供的提现地址应该是和普通 SOL 提款地址相同。

在执行提款 [transfer](#token-transfers) 之前，交易所应检查地址符合 [上文所述](#validating-user-supplied-account-addresses-for-withdrawals) 的规则。

从提款地址为正确的铸币确定关联的代币帐户，并将转账发送到该帐户。  请注意关联的代币帐户现在还不存在，因此交易所应该代表用户为该账户提供资金。  对于 SPL Token v2 账户，为提款账户提供的资金额为 0.00203928 SOL (2,039 280 lamports)。

用来提现的 `spl-token transfer` 命令模板为：
```
$ spl-token transfer --fund-recipient <exchange token account> <withdrawal amount> <withdrawal address>
```

### 其他考虑因素

#### 冻结权限
出于法规合规性原因，SPL 代币发行实体可以为与铸造相关联的所有帐户选择保留“冻结权限”。  这允许他们按照需要将一个给定帐户的资产 [冻结](https://spl.solana.com/token#freezing-accounts)，直到解冻以后才能使用。 如果开放该功能，冻结权限的公钥将在 SPL 代币的铸造账户中注册。

## 测试集成

请务必先在 Solana devnet 和 testnet [clusters](../clusters.md) 测试完整的工作流，然后再迁移到 mainnet-beta 上。 Devnet 是最开放和最灵活、最理想的初始开发方式，而 testnet 提供了更现实的集群配置。 Devnet 和 testnet 都有一个水龙头，您可以通过运行 `solana airdrop 10` 获取一些用来开发和测试的 devnet 或 testnet 的 SOL 代币。
