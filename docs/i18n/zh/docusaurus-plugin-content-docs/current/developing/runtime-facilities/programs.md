---
title: "构建程序"
---

Solana包含少量内置程序，这些程序是运行验证程序节点所必需的。 与第三方程序不同，内置程序是验证程序实现的一部分，可以作为群集升级的一部分进行升级。 可能会进行升级以添加功能，修复错误或提高性能。 个别指令的界面更改很少（如果有的话）发生。 相反，当需要更改时，将添加新指令，并且将先前的指令标记为已弃用。 应用程序可以在自己的时间表上进行升级，而无需担心升级过程中的中断。

对于每个内置程序，将提供每个支持的指令的程序ID和说明。 事务可以混合和匹配来自不同程序的指令，也可以包括来自已部署程序的指令。

## 系统程序

创建帐户并在它们之间转移Lamport

- 程序ID：`11111111111111111111111111111111`
- 说明：[SystemInstruction](https://docs.rs/solana-sdk/VERSION_FOR_DOCS_RS/solana_sdk/system_instruction/enum.SystemInstruction.html)

## 配置程序

将配置数据添加到链和允许对其进行修改的公钥列表中

- 程序ID：`Config1111111111111111111111111111111111111111`
- 说明：[config_instruction](https://docs.rs/solana-config-program/VERSION_FOR_DOCS_RS/solana_config_program/config_instruction/index.html)

与其他程序不同，Config程序未定义任何单独的指令。 它只有一条隐式指令，即“存储”指令。 它的指令数据是一组密钥，用于控制对帐户的访问以及存储在其中的数据。

## 权益计划

创建权益账户并将其委托给验证者

- 程序ID：`Stake11111111111111111111111111111111111111`
- 说明： [StakeInstruction](https://docs.rs/solana-stake-program/VERSION_FOR_DOCS_RS/solana_stake_program/stake_instruction/enum.StakeInstruction.html)

## 投票程序

创建投票账户并对区块进行投票

- 程序ID：`Vote111111111111111111111111111111111111111`
- 说明：[VoteInstruction](https://docs.rs/solana-vote-program/VERSION_FOR_DOCS_RS/solana_vote_program/vote_instruction/enum.VoteInstruction.html)

## BPF加载程序

将程序添加到链中并执行它们。

- 程序ID：`BPFLoader11111111111111111111111111111111111`
- 说明：[LoaderInstruction](https://docs.rs/solana-sdk/VERSION_FOR_DOCS_RS/solana_sdk/loader_instruction/enum.LoaderInstruction.html)

BPF加载程序将其自身标记为它创建的用于存储程序的可执行帐户的“所有者”。 当用户通过程序ID调用指令时，Solana运行时将同时加载您的可执行帐户及其所有者BPF Loader。 然后，运行时将您的程序传递给BPF加载程序以处理指令。

## Secp256k1程序

验证secp256k1公钥恢复操作(ecrecover)。

- 程序ID：`KeccakSecp256k11111111111111111111111111111111`
- 说明：[new_secp256k1_instruction](https://github.com/solana-labs/solana/blob/c1f3f9d27b5f9534f9a37704bae1d690d4335b6b/programs/secp256k1/src/lib.rs#L18)

Secp256k1程序处理一条指令，该指令将在指令数据中序列化的以下结构的计数作为第一个字节：

```
struct Secp256k1SignatureOffsets {
    secp_signature_key_offset: u16,        // offset to [signature,recovery_id,etherum_address] of 64+1+20 bytes
    secp_signature_instruction_index: u8,  // instruction index to find data
    secp_pubkey_offset: u16,               // offset to [signature,recovery_id] of 64+1 bytes
    secp_signature_instruction_index: u8,  // instruction index to find data
    secp_message_data_offset: u16,         // offset to start of message data
    secp_message_data_size: u16,           // size of message data
    secp_message_instruction_index: u8,    // index of instruction data to get message data
}
```

伪代码的操作：
```
process_instruction() {
  for i in 0..count {
      // i'th index values referenced:
      instructions = &transaction.message().instructions
      signature = instructions[secp_signature_instruction_index].data[secp_signature_offset..secp_signature_offset + 64]
      recovery_id = instructions[secp_signature_instruction_index].data[secp_signature_offset + 64]
      ref_eth_pubkey = instructions[secp_pubkey_instruction_index].data[secp_pubkey_offset..secp_pubkey_offset + 32]
      message_hash = keccak256(instructions[secp_message_instruction_index].data[secp_message_data_offset..secp_message_data_offset + secp_message_data_size])
      pubkey = ecrecover(signature, recovery_id, message_hash)
      eth_pubkey = keccak256(pubkey[1..])[12..]
      if eth_pubkey != ref_eth_pubkey {
          return Error
      }
  }
  return Success
}
```

这允许用户在事务中指定用于签名和消息数据的任何指令数据。 通过指定一种特殊的指令sysvar，也可以从事务本身接收数据。

交易成本将计算要验证的签名数乘以签名成本验证乘数。

### 优化注意事项

该操作将必须在(至少部分) 反序列化之后进行，但是所有输入都来自交易数据本身，这使得它相对于交易处理和PoH验证并行执行相对容易。
