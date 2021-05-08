---
title: "交易"
---

程序执行从将[transaction](terminology.md#transaction)提交到集群开始。 Solana运行时将执行一个程序，以按顺序和原子方式处理事务中包含的每个[指令](terminology.md#instruction)。

## 交易剖析

本节介绍事务的二进制格式

### 交易格式

事务包含签名的[compact-array](#compact-array-format)，然后是[message](#message-format)。 签名数组中的每个项目都是给定消息的[数字签名](#signature-format)。 Solana运行时验证签名数是否与[message heade](#message-header-format)的前8位中的数字匹配。 它还验证每个签名是否由与邮件帐户地址数组中相同索引处的公钥相对应的私钥签名。

#### 签名格式

每个数字签名均为ed25519二进制格式，占用64个字节。

### 邮件格式

一条消息包含[header](#message-header-format)，然后是[account address](#account-addresses-format)的紧凑数组，然后是最近的[blockhash](#blockhash-format)，紧接着是[指令](#instruction-format)的紧凑数组。

#### 邮件标题格式

消息头包含三个无符号的8位值。 第一个值是包含交易中所需签名的数量。 第二个值是那些对应的只读帐户地址的数量。 邮件标题中的第三个值是不需要签名的只读帐户地址的数量。

#### 帐户地址格式

要求签名的地址出现在帐户地址数组的开头，其地址首先请求写访问权限，然后请求只读帐户。 不需要签名的地址跟在需要签名的地址之后，再次是先读写帐户，然后是只读帐户。

#### Blockhash区块链哈希值格式

区块哈希包含一个32字节的SHA-256哈希。 它用于指示客户最后一次观察分类帐的时间。 当区块哈希值太旧时，验证程序将拒绝交易。

### 指令格式

一条指令包含一个程序ID索引，后跟一个帐户地址索引的紧凑数组，然后是一个不透明的8位数据的紧凑数组。 程序ID索引用于标识可以解释不透明数据的链上程序。 程序ID索引是消息的帐户地址数组中帐户地址的无符号8位索引。 帐户地址索引是同一数组中的无符号8位索引。

### 紧凑数组格式

紧凑数组被序列化为数组长度，随后是每个数组项。 数组长度是一种特殊的多字节编码，称为compact-u16。

#### Compact-u16格式

一个compact-u16是16位的多字节编码。 第一个字节在其低7位中包含该值的低7位。 如果该值大于0x7f，则设置高位，并将该值的后7位放入第二个字节的低7位。 如果该值大于0x3fff，则设置高位，并将该值的其余2位放入第三个字节的低2位。

### 帐户地址格式

帐户地址是32字节的任意数据。 当地址需要数字签名时，运行时会将其解释为ed25519密钥对的公钥。

## 指示

每个[instruction](terminology.md#instruction)都指定一个程序，应传递给该程序的交易帐户的子集以及一个传递给该程序的数据字节数组。 该程序解释数据数组并在指令指定的帐户上运行。 该程序可以成功返回，或者带有错误代码。 错误返回会导致整个事务立即失败。

程序通常提供帮助程序功能来构造它们支持的指令。 例如，系统程序提供了以下Rust助手来构建[`SystemInstruction::CreateAccount`](https://github.com/solana-labs/solana/blob/6606590b8132e56dab9e60b3f7d20ba7412a736c/sdk/program/src/system_instruction.rs#L63)指令：

```rust
pub fn create_account(
    from_pubkey: &Pubkey,
    to_pubkey: &Pubkey,
    lamports: u64,
    space: u64,
    owner: &Pubkey,
) -> Instruction {
    let account_metas = vec![
        AccountMeta::new(*from_pubkey, true),
        AccountMeta::new(*to_pubkey, true),
    ];
    Instruction::new(
        system_program::id(),
        &SystemInstruction::CreateAccount {
            lamports,
            space,
            owner: *owner,
        },
        account_metas,
    )
}
```

可以在这里找到：

https://github.com/solana-labs/solana/blob/6606590b8132e56dab9e60b3f7d20ba7412a736c/sdk/program/src/system_instruction.rs#L220

### 程序ID

指令的[程序ID](terminology.md#program-id)指定将处理该指令的程序。 程序帐户的所有者指定应使用哪个加载程序来加载和执行程序，并且数据包含有关运行时应如何执行程序的信息。

对于[已部署的BPF程序](developing/on-chain-programs/overview.md)，所有者是BPF加载程序，帐户数据包含BPF字节码。  一旦成功部署，程序帐户便会被加载程序永久标记为可执行文件。 运行时将拒绝指定不可执行程序的事务。


与已部署的程序不同，[runtime facilities](developing/runtime-facilities/programs.md)的处理方式有所不同，因为它们直接内置在Solana运行时中。

### 帐户

指令引用的帐户代表链上状态，并且既作为程序的输入又作为输出。 有关帐户的更多信息，请参见[帐户](accounts.md)章节。

### 指令数据

每个指令都带有一个通用字节数组，该字节数组与帐户一起传递给程序。 指令数据的内容是特定于程序的，通常用于传达程序应执行的操作以及这些操作在帐户所包含的内容之外可能需要的任何其他信息。

程序可以自由指定如何将信息编码到指令数据字节数组中。 数据编码方式的选择应考虑到解码的开销，因为该步骤是由链上程序执行的。 据观察，一些常见的编码(例如Rust的bincode) 效率很低。

[Solana程序库的代币程序](https://github.com/solana-labs/solana-program-library/tree/master/token)提供了一个示例，说明如何有效地对指令数据进行编码，但是请注意，这种方法仅支持固定大小的类型。 代币利用[Pack](https://github.com/solana-labs/solana/blob/master/sdk/program/src/program_pack.rs)特征来对代币指令和代币的指令数据进行编码/解码帐户状态。

## 签名

每笔交易都明确列出了交易指令所引用的所有帐户公钥。 这些公钥的子集每个都带有交易签名。 这些签名向链上程序发出信号，表明帐户持有人已授权交易。 通常，程序使用授权来允许借记帐户或修改其数据。 有关如何将授权传达给程序的更多信息，请参见[帐户](accounts.md#signers)。


## 最近的区块链哈希值

事务包括最近的[blockhash](terminology.md#blockhash)，以防止重复并赋予事务生命周期。 任何与上一个交易完全相同的交易都会被拒绝，因此添加一个更新的区块哈希可以使多个交易重复完全相同的操作。 事务还具有由Blockhash定义的生存期，因为Blockhash太旧的任何事务都将被拒绝。
