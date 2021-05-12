---
title: "Native Programs"
---

Solana contains a small handful of native programs, which are required to run validator nodes. Unlike third-party programs, the native programs are part of the validator implementation and can be upgraded as part of cluster upgrades. 可能会进行升级以添加功能，修复错误或提高性能。 个别指令的界面更改很少（如果有的话）发生。 相反，当需要更改时，将添加新指令，并且将先前的指令标记为已弃用。 应用程序可以在自己的时间表上进行升级，而无需担心升级过程中的中断。

For each native program the program id and description each supported instruction is provided. A transaction can mix and match instructions from different programs, as well include instructions from on-chain programs.

## 系统程序

Create new accounts, allocate account data, assign accounts to owning programs, transfer lamports from System Program owned accounts and pay transacation fees.

- 程序 ID：`11111111111111111111111111111111`
- 说明：[SystemInstruction](https://docs.rs/solana-sdk/VERSION_FOR_DOCS_RS/solana_sdk/system_instruction/enum.SystemInstruction.html)

## 配置程序

将配置数据添加到链和允许对其进行修改的公钥列表中

- 程序 ID：`Config1111111111111111111111111111111111111111`
- 说明：[config_instruction](https://docs.rs/solana-config-program/VERSION_FOR_DOCS_RS/solana_config_program/config_instruction/index.html)

与其他程序不同，Config 程序未定义任何单独的指令。 它只有一条隐式指令，即“存储”指令。 它的指令数据是一组密钥，用于控制对帐户的访问以及存储在其中的数据。

## 权益计划

Create and manage accounts representing stake and rewards for delegations to validators.

- 程序 ID：`Stake11111111111111111111111111111111111111`
- 说明： [StakeInstruction](https://docs.rs/solana-stake-program/VERSION_FOR_DOCS_RS/solana_stake_program/stake_instruction/enum.StakeInstruction.html)

## 投票程序

Create and manage accounts that track validator voting state and rewards.

- 程序 ID：`Vote111111111111111111111111111111111111111`
- 说明：[VoteInstruction](https://docs.rs/solana-vote-program/VERSION_FOR_DOCS_RS/solana_vote_program/vote_instruction/enum.VoteInstruction.html)

## BPF 加载程序

Deploys, upgrades, and executes programs on the chain.

- Program id: `BPFLoaderUpgradeab1e11111111111111111111111`
- 说明：[LoaderInstruction](https://docs.rs/solana-sdk/VERSION_FOR_DOCS_RS/solana_sdk/loader_upgradeable_instruction/enum.UpgradeableLoaderInstruction.html)

The BPF Upgradeable Loader marks itself as "owner" of the executable and program-data accounts it creates to store your program. When a user invokes an instruction via a program id, the Solana runtime will load both your the program and its owner, the BPF Upgradeable Loader. The runtime then passes your program to the BPF Upgradeable Loader to process the instruction.

[More information about deployment](cli/deploy-a-program.md)

## Secp256k1 程序

验证 secp256k1 公钥恢复操作(ecrecover)。

- 程序 ID：`KeccakSecp256k11111111111111111111111111111111`
- 说明：[new_secp256k1_instruction](https://github.com/solana-labs/solana/blob/1a658c7f31e1e0d2d39d9efbc0e929350e2c2bcb/sdk/src/secp256k1_instruction.rs#L31)

Secp256k1 程序处理一条指令，该指令将在指令数据中序列化的以下结构的计数作为第一个字节：

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

这允许用户在事务中指定用于签名和消息数据的任何指令数据。 通过指定一种特殊的指令 sysvar，也可以从事务本身接收数据。

交易成本将计算要验证的签名数乘以签名成本验证乘数。

### 优化注意事项

该操作将必须在(至少部分) 反序列化之后进行，但是所有输入都来自交易数据本身，这使得它相对于交易处理和 PoH 验证并行执行相对容易。
