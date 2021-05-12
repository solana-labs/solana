---
title: 命令のイントロスペクション
---

## 問題

スマートコントラクトプログラムの中には、特定の Message に別の Instruction が存在することを確認したいものがあります。 (例として"secp256k1_instruction"を参照してください)。

## 解決策

プログラムが参照できる新しい sysvar Sysvar1nstructions1111111111111 を追加し、その中に Message の命令データを受信し、さらに現在の命令のインデックスも受信します。

このデータを取り出すために、2 つのヘルパー関数を使うことができます。

```
fn load_current_index(instruction_data: &[u8]) -> u16;
fn load_instruction_at(instruction_index: usize, instruction_data: &[u8]) -> Result<Instruction>;
```

ランタイムはこの特別な命令を認識し、そのための Message 命令データをシリアライズし、さらに現在の命令インデックスを書き込みますので、bpf プログラムはそこから必要な情報を取り出すことができます。

注意：bincode はネイティブコードでは約 10 倍遅く、現在の BPF の命令制限を超えてしまうため、命令のカスタムシリアライズを使用しています。
