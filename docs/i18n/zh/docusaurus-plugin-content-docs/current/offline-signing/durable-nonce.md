---
title: 持久交易随机数（Nonces）
---

持久交易随机数是一个机制，它可以绕过交易典型的个短寿期 [`recent_blockhash`](developing/programming-model/transactions.md#recent-blockhash)。 这些方案是作为Solana方案实施的，其机制详见 [proposal](../implemented-proposals/durable-tx-nonces.md)。

## 使用示例

持久的 nonce CLI 命令的详细使用情况可在 [CLI 引用](../cli/usage.md) 中找到。

### Nonce 授权

可以将对临时帐户的权限分配给另一个帐户。 这样，新授权机构将继承先前的授权机构（包括帐户创建者）对临时帐户的完全控制权。 通过此功能，可以创建更复杂的帐户的所有权安排以及与密钥对无关的派生帐户地址。 `--nonce-authority <AUTHORITY_KEYPAIR>` 参数用于指定此帐户，并且受以下命令：

- `create-nonce-account`
- `new-nonce`
- `withdraw-from-nonce-account`
- `authorize-nonce-account`

### Nonce 帐户创建

持久性交易随机数功能使用一个帐户来存储下一个随机数值。 持久的现时帐户必须为[免租](../implemented-proposals/rent.md#two-tiered-rent-regime)，因此需要最低余额才能实现此目的。

通过首次生成一个新的密钥对，然后在链上创建该帐户来创建一个 nonce 帐户。

- 命令

```bash
solana-keygen new -o nonce-keypair.json
solana create-nonce-account nonce-keypair.json 1
```

- 输出

```text
2SymGjGV4ksPdpbaqWFiDoBz8okvtiik4KE9cnMQgRHRLySSdZ6jrEcpPifW4xUpp4z66XM9d9wM48sA7peG2XL
```

> 要保持密钥对完全离线，请使用 [纸钱包](wallet-guide/paper-wallet.md) 密钥生成 [指令](wallet-guide/paper-wallet.md#seed-phrase-generation)

> [完整使用文档](../cli/usage.md#solana-create-nonce-account)

### 查询存储Nonce值

创建持久的随机数交易需要在签名和提交时将存储的随机数值作为值传递给`--blockhash`参数。 使用以下方法获取当前存储的当前值：

- 命令

```bash
solana nonce none-non-keypair.json
```

- 输出

```text
8GRipryfxcsxN8mAGjy8zbFo9ezaUsh47TsPzmZbuytU
```

> [完整使用文档](../cli/usage.md#solana-get-nonce)

### 提升存储Nonce值

尽管通常不需要在更有用的交易之外进行存储，但存储的当前值可以通过以下方式获取：

- 命令

```bash
solana new-nonce none-non-keypair.json
```

- 输出

```text
44jYe1yPKrjuYDmoFTdgPjg8LFpYyh1PFKJqm5SC1PiSyAL8iw1bhadcAX1SL7KDmREEkmHpYvreKoNv6fZgfvUK
```

> [完整使用文档](../cli/usage.md#solana-new-nonce)

### 显示Nonce账户

以更人性化的格式检查nonce 帐户

- 命令

```bash
solana non-account non-ceypair.json
```

- 输出

```text
balance: 0.5 SOL
minimum balance required: 0.00136416 SOL
nonce: DZar6t2EaCFQTbUP4DHKwZ1wT8gCPW2aRfkVWhydkBvS
```

> [完整使用文档](../cli/usage.md#solana-nonce-account)

### 从Nonce帐号提取资产

通过以下方式从 nonce 帐户提取资产

- 命令

```bash
solana withdraw-from-nonce-account nonce-keypair.json ~/.config/solana/id.json 0.5
```

- 输出

```text
3foNy1SBqwXSsfSfTdmYKDuhnVheRnKXpoPySiUDBVeDEs6iMVokgqm7AqfTjbk7QBE8mqomvMUMNQhtdMvFLide
```

> 通过提取全部余额关闭nonce账户

> [完整使用文档](../cli/usage.md#solana-withdraw-from-nonce-account)

### 为Nonce账户分配新的授权

创建后重新分配 nonce 帐户的授权

- 命令

```bash
solana authorize-non-account non-keypair.json nonce-authority.json
```

- 输出

```text
3F9cg4zN9wHxLGx4c3cUKmqpej4oa67QbALmChsJbfxTgTffRiL3iUehVhR9wQmWgPua66jPuAYeL1K2pYYjbNoT
```

> [完整使用文档](../cli/usage.md#solana-authorize-nonce-account)

## 支持持久Nonce的其他命令

要将持久随机数与其他CLI子命令一起使用，必须支持两个参数。

- `--nonce`，指定帐户存储 nonce 值
- `--nonce-authority`，指定一个可选的 [nonce authority](#nonce-authority)

到目前为止，以下子命令已接受此处理

- [`支付`](../cli/usage.md#solana-pay)
- [`委托质押`](../cli/usage.md#solana-delegate-stake)
- [`停用质押`](../cli/usage.md#solana-deactivate-stake)

### 使用持久Nonce的支付示例

在这里，我们演示了Alice使用持久 nonce 向Bob 1 SOL支付的费用。 对于支持持久随机数的所有子命令，该过程相同

#### - 创建帐户

首先，我们需要为Alice、Alice的none和Bob准备一些账户

```bash
$ solana-keygen new -o alice.json
$ solana-keygen new -o nonce.json
$ solana-keygen new -o bob.json
```

#### - Alice账户充值

Alice 需要一些资产来创建一个 nonce 帐户并发送给 Bob。 空投一些SOL给她

```bash
$ solana airdrop -k alice.json 10
10 SOL
```

#### - 创建 Alice 的 nonce 帐户

现在Alice需要一个nonce 帐户。 创建一个

> 这里没有单独的 [nonce authority](#nonce-authority) 被使用，所以 `alice.json` 对nonce 帐户拥有完全的权限

```bash
$ solana create-nonce-account -k alice.json nonce.json 1
3KPZr96BTsL3hqera9up82KAU462Gz31xjqJ6ehuAjF935Yf8i1kmfEbo6SVbNaACKE5z6gySrNjVRvmS8DcPuwV
```

#### - 支付给 Bob 的首次失败尝试

Alice 试图为支付给 Bob，但签名需要太长时间。 指定的区块哈希已经过期，导致交易失败

```bash
$ solana pay -k alice.json --blockhash expiredDTaxfagttWjQweib42b6ZHADSx94Tw8gHx3W7 bob.json 1
[2020-01-02T18:48:28.462911000Z ERROR solana_cli::cli] Io(Custom { kind: Other, error: "Transaction \"33gQQaoPc9jWePMvDAeyJpcnSPiGUAdtVg8zREWv4GiKjkcGNufgpcbFyRKRrA25NkgjZySEeKue5rawyeH5TzsV\" failed: None" })
Error: Io(Custom { kind: Other, error: "Transaction \"33gQQaoPc9jWePMvDAeyJpcnSPiGUAdtVg8zREWv4GiKjkcGNufgpcbFyRKRrA25NkgjZySEeKue5rawyeH5TzsV\" failed: None" })
```

#### - 用 Nonce 来补救！

Alice 重试交易，这次指定她的nonce账户和存储在那里的区块哈希。

> 记住，`alice.json` 是这个示例中的 [nonce 授权](#nonce-authority)

```bash
$ solana nonce-account nonce.json
balance: 1 SOL
minimum balance required: 0.00136416 SOL
nonce: F7vmkY3DTaxfagttWjQweib42b6ZHADSx94Tw8gHx3W7
```

```bash
$ solana pay -k alice.json --blockhash F7vmkY3DTaxfagtWjQweib42b6ZHADSx94Tw8gHx3W7 --nonce nonce.json bob.json 1
HR1368UKHVZyenmH7yVz5sBAijV6XAPeWbEiXEGVYQorRMcoijeNAbzZqEZiH8cDB8tk65ckeegFjK8dHwNFgQ
```

#### - 成功了！

交易成功！ Bob 从 Alice 那里收到1个SOL，并且Alice存储的nonce更新到了一个新的值

```bash
$ solana balance -k bob.json
1 SOL
```

```bash
$ solana nonce-account nonce.json
balance: 1 SOL
minimum balance required: 0.00136416 SOL
nonce: 6bjroqDcZgTv6Vavhqf81oBHTv3aMnX19UTB51YhAZnN
```
