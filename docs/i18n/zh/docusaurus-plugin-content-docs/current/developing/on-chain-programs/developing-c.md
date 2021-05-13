---
title: "用 C 语言开发"
---

Solana 支持使用 C 和 C++ 语言编写链上的程序。

## 项目布局 {#project-layout}

C 项目规定如下：

```
/src/<program name>
/makefile
```

`makefile` 应该包含以下内容：

```bash
OUT_DIR := <path to place to resulting shared object>
include ~/.local/share/solana/install/active_release/bin/sdk/bpf/c/bpf.mk
```

Bpf-sdk可能不在上面指定的确切位置，但是如果您根据[如何开发](#how-to-build)来设置环境，那么就是这样。

来看一下的 C 程序的[helloworld](https://github.com/solana-labs/example-helloworld/tree/master/src/program-c)示例。

## 如何开发 {#how-to-build}

首先设置环境：
- 从https://rustup.rs安装最新的Rust稳定版本
- 从https://docs.solana.com/cli/install-solana-cli-tools安装最新的Solana命令行工具

然后使用make构建：
```bash
make -C <program directory>
```

## 如何测试 {#how-to-test}

Solana 使用 [Criterion](https://github.com/Snaipe/Criterion) 测试框架，并且在每次构建程序时都会执行测试，[如何开发](#how-to-build)。

要添加测试，请在源文件`test_<program
name>.c`旁边创建一个新文件，并使用标准测试用例填充它。  有关示例，请参见[helloworld C测试](https://github.com/solana-labs/example-helloworld/blob/master/src/program-c/src/helloworld/test_helloworld.c)或[Criterion文档](https://criterion.readthedocs.io/en/master)，获取编写测试用例的信息。

## 程序入口点 {#program-entrypoint}

程序导出一个已知的入口点符号，在调用程序时，Solana运行时将查找并调用该入口点符号。  Solana支持多个[BPF加载程序版本](overview.md#versions)，它们之间的入口点可能会有所不同。 程序必须为相同的加载器编写并部署。  有关更多详细信息，请参见[概览](overview#loaders)。

当前有两个受支持的加载器：[BPF加载器](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/bpf_loader.rs#L17)和[已弃用BFT加载器](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/bpf_loader_deprecated.rs#L14)。

它们都有相同的原始入口点定义，以下是运行时查找和调用的原始符号：

```c
extern uint64_t entrypoint(const uint8_t *input)
```

该入口点采用通用字节数组，其中包含序列化的程序参数(程序ID，帐户，指令数据等)。  为了反序列化参数，每个加载器都包含其自己的[帮助器函数](#Serialization)。

请参阅 [使用入口点的简单实例](https://github.com/solana-labs/example-helloworld/blob/bc0b25c0ccebeff44df9760ddb97011558b7d234/src/program-c/src/helloworld/helloworld.c#L37)，来看看它们是如何配合使用的。

### 序列化 {#serialization}

请参阅[helloworld对反序列化功能的使用](https://github.com/solana-labs/example-helloworld/blob/bc0b25c0ccebeff44df9760ddb97011558b7d234/src/program-c/src/helloworld/helloworld.c#L43)。

每个加载程序都提供一个帮助程序功能，该功能将程序的输入参数反序列化为 C 类型：
- [BPF加载器反序列化](https://github.com/solana-labs/solana/blob/d2ee9db2143859fa5dc26b15ee6da9c25cc0429c/sdk/bpf/c/inc/solana_sdk.h#L304)
- [BPF 加载器已弃用的反序列化](https://github.com/solana-labs/solana/blob/8415c22b593f164020adc7afe782e8041d756ddf/sdk/bpf/c/inc/deserialize_deprecated.h#L25)

某些程序可能希望自己执行序列化，并且可以通过提供其自己的[原始入口点](#program-entrypoint)实现来实现。 请注意，提供的反序列化功能会将引用保留回序列化字节数组，以引用允许程序修改的变量(lamport，帐户数据)。  这样做的原因是，在返回时，加载程序将读取这些修改，以便可以将其提交。  如果程序实现其自己的反序列化功能，则需要确保将程序希望进行的所有修改都写回到输入字节数组中。

有关加载程序如何序列化程序输入的详细信息，请参见[Input Parameter Serialization](overview.md#input-parameter-serialization)文档。

## 数据类型 {#data-types}

加载程序的反序列化助手函数将填充[SolParameters](https://github.com/solana-labs/solana/blob/8415c22b593f164020adc7afe782e8041d756ddf/sdk/bpf/c/inc/solana_sdk.h#L276)结构：

```c
/**
 * 程序进入点输入数据被反序列化的结构。
 */
typef structt volt_
  SolAccountInfo* ka; /** 指向SolAccountInfo阵列的指针， 必须已经
                          指向一个 SolAccountInfos */
  uint64_t ka_num; /** `ka`中的 SolAccountInfo 条目数 */
  const uint8_t *数据； /** 指示数据指针*/
  uint64_t data_len; /** 指令数据字节长度 */
  const SolPubkey *program_id; /** 当前正在执行的程序 */
} Solameters;
```

“ ka”是指令引用帐户的有序数组，并表示为[SolAccountInfo](https://github.com/solana-labs/solana/blob/8415c22b593f164020adc7afe782e8041d756ddf/sdk/bpf/c/inc/solana_sdk.h#L173)结构。  帐户在数组中的位置表示其含义，例如，在转移lamports时，一条指令可以将第一个帐户定义为源，将第二个帐户定义为目的地。

`AccountInfo`结构的成员是只读的，但`lamports`和`data`除外。  程序都可以根据[runtime执行策略](developing/programming-model/accounts.md#policy)对两者进行修改。  当一条指令多次引用相同的帐户时，数组中可能有重复的`SolAccountInfo`条目，但它们都指向原来的输入字节数组。  程序应谨慎处理这些情况，以避免对同一缓冲区的读/写重叠。  如果程序实现其自己的反序列化功能，则应注意适当地处理重复帐户。

`数据`是正在处理的[指令的指令数据](developing/programming-model/transactions.md#instruction-data)中的通用字节数组。

`program_id`是当前正在执行的程序的公钥。

## 堆（Heap）{#heap}

C 程序可以通过系统调用[`calloc`](https://github.com/solana-labs/solana/blob/c3d2d2134c93001566e1e56f691582f379b5ae55/sdk/bpf/c/inc/solana_sdk.h#L245)或者通过虚拟的 32 Kb heap 区域顶部实现它们自己的堆地址 x300000000。  堆区域也被 `calloc` 使用，因此如果一个程序实现了自己的堆，它不应该同时调用 `calloc`。

## 日志 {#logging}

运行时提供了两个系统调用，这些系统调用将获取数据并将其记录到程序日志中。

- [`sol_log(const char*)`](https://github.com/solana-labs/solana/blob/d2ee9db2143859fa5dc26b15ee6da9c25cc0429c/sdk/bpf/c/inc/solana_sdk.h#L128)
- [`sol_log_64(uint64_t, uint64_t, uint64_t, uint64_t, uint64_t)`](https://github.com/solana-labs/solana/blob/d2ee9db2143859fa5dc26b15ee6da9c25cc0429c/sdk/bpf/c/inc/solana_sdk.h#L134)

[调试](debugging.md#logging) 章节有更多关于程序日志工作的信息。

## 计算预算 {#compute-budget}

使用系统调用[`sol_log_compute_units()`](https://github.com/solana-labs/solana/blob/d3a3a7548c857f26ec2cb10e270da72d373020ec/sdk/bpf/c/inc/solana_sdk.h#L140)记录包含剩余编号的消息暂停执行之前程序可能消耗的计算单元数。

相关的更多信息，请参见[计算预算](developing/programming-model/runtime.md#compute-budget)。

## ELF转储 {#elf-dump}

可以将BPF共享对象的内部信息转储到文本文件中，以更深入地了解程序的组成及其在运行时的工作方式。  转储将包含ELF信息以及所有符号和实现它们的指令的列表。  一些BPF加载程序的错误日志消息将引用发生错误的特定指令号。 可以在ELF转储中查找这些引用，以标识有问题的指令及其上下文。

创建一个转储文件：

```bash
$ cd <program directory>
$ make dump_<program name>
```

## 示例 {#examples}

[Solana 程序库github](https://github.com/solana-labs/solana-program-library/tree/master/examples/c)代码库包含了 C 语言的例子集合。
