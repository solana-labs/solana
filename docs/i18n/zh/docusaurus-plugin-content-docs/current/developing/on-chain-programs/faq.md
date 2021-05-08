---
title: "常见问题解答"
---

在编写或与Solana程序进行交互时，经常会遇到一些常见的问题或困难。 以下是有助于回答这些问题的资源。

如果还没有解决您的问题，那么Solana[#developers](https://discord.gg/RxeGBH)Discord频道是一个不错的资源。

## `CallDepth`错误

此错误意味着跨程序调用超出了允许的调用深度。

请参见[跨程序调用调用深度](developing/programming-model/calling-between-programs.md#call-depth)

## `CallDepthExceeded`错误

此错误表示已超出BPF堆栈深度。

请参阅[通话深度](overview.md#call-depth)

## 计算约束

请参见[计算约束](developing/programming-model/runtime.md#compute-budget)

## 浮动Rust类型

请参见[浮动支持](overview.md#float-support)

## Heap大小

请参见[heap](overview.md#heap)

## 无效账户数据

该程序错误的发生可能有很多原因。 通常，这是由于在指令中的错误位置或与正在执行的指令不兼容的帐户向程序传递了程序不期望的帐户所致。

当执行跨程序指令而忘记提供您正在调用的程序的帐户时，程序的实现也可能导致此错误。

## 无效指示数据

尝试反序列化指令时，可能会发生此程序错误，请检查传入的结构是否与指令完全匹配。 字段之间可能会有一些填充。 如果程序实现了Rust的`Pack`特性，则尝试打包和解压缩指令类型`T`以确定程序期望的确切编码：

https://github.com/solana-labs/solana/blob/v1.4/sdk/program/src/program_pack.rs

## MissingRequiredSignature

有些说明要求帐户必须是签名者；如果预计将对帐户进行签名但未签名，则返回此错误。

当执行需要签名程序地址的跨程序调用时，程序的实现也可能会导致此错误，但是传递的签名者种子将传递给[`invoke_signed`](developing/programming-model/calling-between-programs.md)与用于创建程序地址[`create_program_address`](developing/programming-model/calling-between-programs.md#program-derived-addresses)的签名者种子不匹配。

## `rand` Rust依赖导致编译失败

请参见[Rust项目依赖项](developing-rust.md#project-dependencies)

## Rust限制

请参见[Rust限制](developing-rust.md#restrictions)

## 堆栈大小

请参见[stack](overview.md#stack)