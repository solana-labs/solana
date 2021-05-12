---
title: 指令反省
---

## 面临的问题

一些智能合约程序可能想要验证另一个指令是否存在于给定的消息中，因为该指令可能是在预编译函数中执行某些数据的验证。 (参见secp256k1/_instruction的例子)。

## 解决方案

增加一个新的sysvar Sysvar1nstructions1111111111111111111111111，程序可以在里面引用和接收消息的指令数据，也可以引用当前指令的索引。

可以使用两个辅助函数来提取这些数据：

```
fn load_current_index(instruction_data: &[u8]) -> u16;
fn load_instruction_at(instruction_index: usize, instruction_data: &[u8]) -> Result<Instruction>;
```

运行时将识别这条特殊的指令，为其序列化消息指令数据，同时写入当前的指令索引，然后bpf程序就可以从中提取必要的信息。

注意：使用自定义序列化指令是因为二进制码在原生代码中的速度要慢10倍左右，而且超过了当前bpf指令的限制。
