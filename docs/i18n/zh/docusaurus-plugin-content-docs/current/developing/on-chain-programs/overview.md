---
title: "概述"
---

开发人员可以编写自己的程序并将其部署到Solana区块链。

[Helloworld示例](examples.md#helloworld)是了解如何编写、构建、部署、与链上程序交互的入门材料。

## Berkley数据包过滤器(BPF)

Solana链上程序通过[LLVM编译器基础结构](https://llvm.org/)编译为[可执行和可链接格式(ELF)](https://en.wikipedia.org/wiki/Executable_and_Linkable_Format)，其中包含了[Berkley数据包过滤器(BPF)](https://en.wikipedia.org/wiki/Berkeley_Packet_Filter)字节码的变体。

由于Solana使用LLVM编译器基础结构，因此可以使用可以针对LLVM的BPF后端的任何编程语言编写程序。 Solana当前支持用Rust和C / C ++编写程序。

BPF提供了有效的[指令集](https://github.com/iovisor/bpf-docs/blob/master/eBPF.md)，可以在解释的虚拟机中执行，也可以作为高效的即时编译原生执行指令。

## 内存映射

Solana BPF程序使用的虚拟地址内存映射是固定的，其布局如下

- 程序代码从0x100000000开始
- 堆栈数据从0x200000000开始
- 堆数据从0x300000000开始
- 程序输入参数从0x400000000开始

上面的虚拟地址是起始地址，但是程序可以访问存储器映射的子集。  如果程序尝试读取或写入未授予其访问权限的虚拟地址，则会panic，并且将返回`AccessViolation`错误，其中包含尝试违反的地址和大小。

## 堆栈 {#stack}

BPF使用堆栈帧而不是可变堆栈指针。 每个堆栈帧的大小为4KB。

如果程序违反了该堆栈帧大小，则编译器将报告溢出情况，作为警告。

例如：`Error: Function
_ZN16curve25519_dalek7edwards21EdwardsBasepointTable6create17h178b3d2411f7f082E
Stack offset of -30728 exceeded max offset of -4096 by 26632 bytes, please
minimize large stack variables`

该消息标识哪个符号超出了其堆栈框架，但是如果它是Rust或C ++符号，则名称可能会被修饰。  要对Rust符号进行解码，请使用[rustfilt](https://github.com/luser/rustfilt)。  上面的警告来自Rust程序，因此，已取消组合的符号名称为：

```bash
$ rustfilt _ZN16curve25519_dalek7edwards21EdwardsBasepointTable6create17h178b3d2411f7f082E
curve25519_dalek::edwards::EdwardsBasepointTable::create
```

要对C ++符号进行解码，请使用binutils中的`c++filt`。

报告警告而不提示错误的原因是，即使程序不使用该功能，某些从属工具也可能包含违反堆栈框架限制的功能。 如果程序在运行时违反了堆栈大小，则会报告`AccessViolation`错误。

BPF堆栈帧占用一个从0x200000000开始的虚拟地址范围。

## 调用深度

程序被限制为必须快速运行，并且为了方便起见，程序的调用堆栈被限制为最大深度为64帧。

## 堆（Heap）

程序可以直接在C中或通过Rust `alloc` API来访问运行时堆。 为了促进快速分配，使用了一个简单的32KB凹凸堆。 堆不支持`free`或`realloc`，因此请慎重使用它。

在内部，程序可以访问从虚拟地址0x300000000开始的32KB内存区域，并且可以根据程序的特定需求实现自定义堆。

- [Rust程序堆使用情况](developing-rust.md#heap)
- [C程序堆使用情况](developing-c.md#heap)

## 浮点数支持

程序支持Rust的float操作的有限子集，尽管由于涉及的开销而强烈不建议使用。 如果程序尝试使用不受支持的浮点运算，则运行时将报告未解决的符号错误。

## 静态可写入数据

程序共享对象不支持可写共享数据。  使用相同的共享只读代码和数据在多个并行执行之间共享程序。 这意味着开发人员不应在程序中包含任何静态可写变量或全局变量。 将来，可以添加写时复制机制以支持可写数据。

## 签名分配

BPF 指令集不支持 [签名分配](https://www.kernel.org/doc/html/latest/bpf/bpf_design_QA.html#q-why-there-is-no-bpf-sdiv-for-signed-divide-operation)。 添加签名分配的指令是一个考虑因素。

## 加载程序（loader）

程序由运行时加载程序部署并执行，目前有两个受支持的加载程序[BPF加载程序](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/bpf_loader.rs#L17)]和[不建议使用BPF加载程序](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/bpf_loader_deprecated.rs#L14)

加载程序可能支持不同的应用程序二进制接口，因此开发人员必须为其编写程序并将其部署到同一加载程序中。  如果为一个装载程序编写的程序被部署到另一个装载程序，则由于程序输入参数的反序列化不匹配，结果通常是`AccessViolation`错误。

出于所有实际目的，应始终将程序编写为以最新的BPF加载程序为目标，并且最新的加载程序是命令行界面和javascript API的默认设置。

有关为特定加载程序实现程序的语言特定信息，请参见：
- [Rust程序入口点](developing-rust.md#program-entrypoint)
- [C程序入口点](developing-c.md#program-entrypoint)

### 部署

BPF程序部署是将BPF共享对象上载到程序帐户的数据中并标记该帐户可执行文件的过程。  客户端将BPF共享对象分成较小的部分，并将其作为[`Write`](https://github.com/solana-labs/solana/blob/bc7133d7526a041d1aaee807b80922baa89b6f90/sdk/program/src/loader_instruction.rs#L13)的指令数据发送向加载程序的指令，加载程序在此将数据写入程序的帐户数据。  一旦收到所有片段，客户端就会向加载程序发送[`Finalize`](https://github.com/solana-labs/solana/blob/bc7133d7526a041d1aaee807b80922baa89b6f90/sdk/program/src/loader_instruction.rs#L30)指令，然后加载程序将验证BPF数据是否有效，并将程序帐户标记为_executable_。  一旦程序帐户被标记为可执行，随后的交易就可以发出该程序要处理的指令。

当指令针对可执行的BPF程序时，加载程序将配置程序的执行环境，序列化程序的输入参数，调用程序的入口点，并报告遇到的任何错误。

有关更多信息，请参见[deploying](deploying.md)

### 输入参数序列化

BPF加载程序将程序输入参数序列化为字节数组，然后将其传递到程序的入口点，由程序负责在链上反序列化它。  不赞成使用的加载器和当前加载器之间的变化之一是，输入参数以某种方式序列化，导致各种参数落在对齐字节数组内的对齐偏移量上。  这允许反序列化实现直接引用字节数组并提供指向程序的对齐指针。

有关序列化的特定于语言的信息，请参见：
- [Rust程序参数反序列化](developing-rust.md#parameter-deserialization)
- [C程序参数反序列化](developing-c.md#parameter-deserialization)

最新的加载器按如下方式序列化程序输入参数(所有编码均为小尾数法)：

- 8字节无符号帐户数
- 对于每个帐户
  - 1个字节，指示这是否是重复帐户，如果不是重复帐户，则值为0xff，否则值为与其重复的帐户的索引。
  - 填充7个字节
    - 如果不重复
      - 1个字节的填充
      - 1个字节的布尔值，如果account是签名者，则为true
      - 1字节布尔值，如果帐户可写，则为true
      - 1字节布尔值，如果帐户可执行，则为true
      - 4个字节的填充
      - 32个字节的帐户公钥
      - 该帐户的所有者公共密钥的32个字节
      - 该帐户拥有的Lamport的8字节无符号数
      - -8个字节的无符号帐户数据字节数
      - x字节的帐户数据
      - 10k字节的填充，用于重新分配
      - 足够的填充以将偏移量对齐到8个字节。
      - 8个字节的租用纪元
- 8个字节的无符号指令数据
- -x字节的指令数据
- -程序ID的32个字节
