---
title: Rust 客户端
---

## 面临的问题

诸如Bench-tps之类的高级测试是根据`客户端`特征编写的。 将这些测试作为测试套件的一部分执行时，我们将使用低级的`BankClient`实现。 当需要对集群运行相同的测试时，我们使用`ThinClient（轻客户端）`实现。 这种方法的问题在于，它意味着该特性将不断扩展，从而包括新的实用程序功能，并且其所有实现都需要添加新功能。 通过将面向用户的对象与抽象化网络接口的特征分离，我们可以扩展面向用户的对象，以包括各种有用的功能，例如RpcClient的“spinner”，而无需担心扩展特征及其实现。

## 拟定的解决方案

代替实现`客户端`特征，应使用其实现来构造`ThinClient`。 这样，当前具有`客户端`特征的所有实用程序功能都可以移至`ThinClient`中。 然后，`ThinClient`可以移至`solana-sdk`中，因为它的所有网络相关性都在`Client`的实现中。 然后，我们将添加一个名为`Client`的新实现，称为`ClusterClient`，并将其存在于`ThinClient`当前所在的`Solana-client`工具箱中。

重组之后，任何需要客户端的代码都将以`ThinClient`的形式编写。 在单元测试中，将使用`ThinClient<BankClient>`调用该功能，而`main()`函数、基准测试和集成测试将通过`ThinClient<ClusterClient>`调用该功能。

如果更高级别的组件需要比`BankClient`能够实现的功能更广，则应按照与此处所述相同的模式，通过实现第二个特征的第二个对象来实现。

### 错误处理

`客户端`应使用现有的`TransportError`枚举来进行错误处理，除了`Custom(String)`字段应更改为`Custom(Box<dyn Error>)`。

### 实施战略

1. 在`solana-sdk`、`RpcClientTng`中添加新对象，其中`Tng`后缀是临时的，代表“TheNextGeneration”
2. 用`SyncClient`的实现来初始化`RpcClientTng`。
3. 将新对象添加到`solana-sdk`、`ThinClientTng`中；使用`RpcClientTng`和`AsyncClient`实现对其进行初始化
4. 将所有单元测试从`BankClient`移至`ThinClientTng<BankClient>`
5. 添加`ClusterClient`
6. 将`ThinClient`用户移到`ThinClientTng<ClusterClient>`
7. 删除`ThinClient`并将`ThinClientTng`重命名为`ThinClient`
8. 将`RpcClient`用户移到新的`ThinClient<ClusterClient>`
9. 删除`RpcClient`并将`RpcClientTng`重命名为`RpcClient`
