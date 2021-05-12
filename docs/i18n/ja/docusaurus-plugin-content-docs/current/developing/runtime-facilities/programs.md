---
title: "Native Programs"
---

Solana contains a small handful of native programs, which are required to run validator nodes. Unlike third-party programs, the native programs are part of the validator implementation and can be upgraded as part of cluster upgrades. アップグレードの目的は、"機能の追加"、"バグの修正"、"パフォーマンスの向上"などです。 個々の命令のインターフェイスを変更することはあったとしても、ほとんどありません。 その代わり、変更が必要な場合は新しい命令が追加され、以前の命令は非推奨とされます。 アプリは独自のスケジュールでアップグレードすることができ、アップグレード中に不具合が発生する心配はありません。

For each native program the program id and description each supported instruction is provided. A transaction can mix and match instructions from different programs, as well include instructions from on-chain programs.

## システムプログラム

Create new accounts, allocate account data, assign accounts to owning programs, transfer lamports from System Program owned accounts and pay transacation fees.

- Program id: `11111111111111111111111111111111`
- 命令: [システム命令](https://docs.rs/solana-sdk/VERSION_FOR_DOCS_RS/solana_sdk/system_instruction/enum.SystemInstruction.html)

## コンフィグプログラム

チェーンに設定データを追加し、その変更を許可された公開キーのリストを追加します。

- プログラム ID: `Config11111111111111111111111111111`
- 命令: [config_instruction](https://docs.rs/solana-config-program/VERSION_FOR_DOCS_RS/solana_config_program/config_instruction/index.html)

他のプログラムと異なり、コンフィグプログラムは個々の命令を定義していません。 暗黙の命令である"ストア"命令を 1 つだけ持っています。 その命令データは、アカウントへのアクセスを制御するキーのセットと、そこに保存するデータです。

## ステーキングプログラム

Create and manage accounts representing stake and rewards for delegations to validators.

- プログラム Id: `Stake11111111111111111111111111111111111111`
- 命令: [StakeInstruction](https://docs.rs/solana-stake-program/VERSION_FOR_DOCS_RS/solana_stake_program/stake_instruction/enum.StakeInstruction.html)

## 投票プログラム

Create and manage accounts that track validator voting state and rewards.

- プログラム ID: `Vote1111111111111111111111111111111`
- 命令: [VoteInstruction](https://docs.rs/solana-vote-program/VERSION_FOR_DOCS_RS/solana_vote_program/vote_instruction/enum.VoteInstruction.html)

## BPF ローダー

Deploys, upgrades, and executes programs on the chain.

- Program id: `BPFLoaderUpgradeab1e11111111111111111111111`
- 命令: [LoaderInstruction](https://docs.rs/solana-sdk/VERSION_FOR_DOCS_RS/solana_sdk/loader_upgradeable_instruction/enum.UpgradeableLoaderInstruction.html)

The BPF Upgradeable Loader marks itself as "owner" of the executable and program-data accounts it creates to store your program. When a user invokes an instruction via a program id, the Solana runtime will load both your the program and its owner, the BPF Upgradeable Loader. The runtime then passes your program to the BPF Upgradeable Loader to process the instruction.

[More information about deployment](cli/deploy-a-program.md)

## Secp256k1 プログラム

Secp256k1 public key recovery operations (ecrecover).

- プログラム id: `KeccakSecp256k11111111111111111111111111111`
- 命令: [new_secp256k1_instruction](https://github.com/solana-labs/solana/blob/1a658c7f31e1e0d2d39d9efbc0e929350e2c2bcb/sdk/src/secp256k1_instruction.rs#L31)

"secp256k1"プログラムでは，1 バイト目に以下の構造体を取り込んだ命令を処理します。命令データにシリアライズされた以下の構造体のカウントを 1 バイト目として取り込む命令を処理します。

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

操作の疑似コード

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

これにより、署名やメッセージのデータとして、トランザクション内の任意の命令データを指定することができます。 また、特別な命令 sysvar を指定することで、トランザクション自体からデータを受け取ることもできます。

トランザクションのコストは、検証する署名の数に署名コスト検証乗数を乗じてカウントされます。

### オプティマイゼーションノート

この操作は、(少なくとも部分的な)"デシリアライゼーションの後に行われる必要がありますが、すべての入力はトランザクションデータ自体から得られるため、トランザクション処理や PoH 検証と並行して比較的容易に実行することができます。
