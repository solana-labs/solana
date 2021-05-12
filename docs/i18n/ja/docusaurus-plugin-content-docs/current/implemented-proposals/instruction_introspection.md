---
title: 命令のイントロスペクション
---

## 問題

スマートコントラクトプログラムの中には、特定のMessageに別のInstructionが存在することを確認したいものがあります。 (例として"secp256k1\_instruction"を参照してください)。

## 解決策

プログラムが参照できる新しい sysvar Sysvar1nstructions1111111111111 を追加し、その中に Message の命令データを受信し、さらに現在の命令のインデックスも受信します。

このデータを取り出すために、2つのヘルパー関数を使うことができます。

```
fn load_current_index(instruction_data: &[u8]) -> u16;
fn load_instruction_at(instruction_index: usize, instruction_data: &[u8]) -> Result<Instruction>;
```

ランタイムはこの特別な命令を認識し、そのためのMessage命令データをシリアライズし、さらに現在の命令インデックスを書き込みますので、bpfプログラムはそこから必要な情報を取り出すことができます。

注意：bincodeはネイティブコードでは約10倍遅く、現在のBPFの命令制限を超えてしまうため、命令のカスタムシリアライズを使用しています。
