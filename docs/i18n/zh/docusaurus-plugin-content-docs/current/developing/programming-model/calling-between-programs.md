---
title: 程序之间的调用
---

## 跨程序调用 {#cross-program-invocations}

Solana运行时允许程序通过称为跨程序调用的机制相互调用。  程序之间的调用是通过一个程序调用另一个程序的指令来实现的。  调用程序将暂停，直到被调用的程序完成对指令的处理为止。

例如，客户可以创建一个交易来修改两个帐户，每个帐户都由单独的链上程序拥有：

```rust,ignore
let message = Message::new(vec![
    token_instruction::pay(&alice_pubkey),
    acme_instruction::launch_missiles(&bob_pubkey),
]);
client.send_and_confirm_message(&[&alice_keypair, &bob_keypair], &message);
```

客户可以代之以允许`acme`程序代表客户方便地调用`token`指令：

```rust,ignore
let message = Message::new(vec![
    acme_instruction::pay_and_launch_missiles(&alice_pubkey, &bob_pubkey),
]);
client.send_and_confirm_message(&[&alice_keypair, &bob_keypair], &message);
```

给定两个链上程序`token`和`acme`，每个程序分别执行指令`pay()`和`launch_missiles()`，可以通过调用`token`模块中定义的函数来实现acme跨程序调用：

```rust,ignore
mod acme {
    use token_instruction;

    fn launch_missiles(accounts: &[AccountInfo]) -> Result<()> {
        ...
    }

    fn pay_and_launch_missiles(accounts: &[AccountInfo]) -> Result<()> {
        let alice_pubkey = accounts[1].key;
        let instruction = token_instruction::pay(&alice_pubkey);
        invoke(&instruction, accounts)?;

        launch_missiles(accounts)?;
    }
```

Solana的运行时内置了`invoke()`，它负责通过指令的`program_id`字段将给定指令路由到`token`程序。

请注意，`invoke` 要求调用者传递被调用指令所需的所有帐户。  这意味着可执行帐户(与指令的程序ID匹配的帐户) 和传递给指令处理器的帐户都可以。

在调用`pay()`之前，运行时必须确保`acme`没有修改`token`拥有的任何帐户。 它通过在`acme`调用`invoke`时将运行时策略应用于帐户的当前状态，而不是在`acme`指令开始时将帐户的初始状态应用到帐户的当前状态。 在`pay()`完成之后，运行时必须再次通过应用运行时的策略来确保`token`不会修改`acme`拥有的任何帐户，但是这次使用`token`程序ID。 最后，在`pay_and_launch_missiles()`完成之后，运行时必须再次使用runtime 策略，这通常是正常的时间，但要使用所有更新的`pre_ *`变量。 如果执行直到`pay()`为止的`pay_and_launch_missiles()`没有任何无效的帐户更改，`pay()`没有任何无效的更改，并且从`pay()`一直执行到`pay_and_launch_missiles()`返回，则没有无效的更改，然后，runtime可以过渡性地假设`pay_and_launch_missiles()`总体上没有进行无效的帐户更改，因此可以提交所有这些帐户修改。

### 需要特权的指令

运行时使用授予调用者程序的特权来确定可以扩展给被调用者的特权。 在这种情况下，特权是指签名者和可写帐户。 例如，如果调用者正在处理的指令包含签名者或可写帐户，则调用者可以调用也包含该签名者和/或可写帐户的指令。

此特权扩展依赖于程序是不可变的这一事实。 对于`acme`程序，运行时可以安全地将事务签名视为`token`指令签名。 当运行时看到`token`指令引用`alice_pubkey`时，它将在`acme`指令中查找密钥，以查看该密钥是否与已签名的帐户相对应。 在这种情况下，它会这样做并因此授权`token`程序修改Alice的帐户。

### 程序签名帐户

程序可以使用[程序派生地址](#program-derived-addresses)发出包含未在原始交易中签名的已签名帐户的指令。

为了用程序派生的地址签署一个帐户，程序可以`invoke_signed()`。

```rust,ignore
        invoke_signed(
            &instruction,
            accounts,
            &[&["First addresses seed"],
              &["Second addresses first seed", "Second addresses second seed"]],
        )?;
```

### 调用深度

跨程序调用允许程序直接调用其他程序，但当前深度限制为4。

### 可重入

目前，可重入仅限于以固定深度为上限的直接自递归。 此限制可防止程序可能在不知道稍后会被调用回状态的情况下从中间状态调用另一个程序的情况。 直接递归可以使程序在被调用时完全控制其状态。

## 程序派生地址

程序派生的地址允许在[程序之间调用](#cross-program-invocations)时使用以编程方式生成的签名。

使用程序派生的地址，可以向某个程序授予某个帐户的权限，然后再将该权限转移给另一个帐户。 这是可能的，因为该程序可以在授予权限的事务中充当签名者。

例如，如果两个用户想要对Solana中的游戏结果押注，则他们每个人都必须将其下注的资产转移到将遵守协议的某些中介机构上。 当前，在Solana中尚无办法将此中介程序作为程序来实现，因为该中介程序无法将资产转让给获胜者。

对于许多DeFi应用程序来说，此功能是必需的，因为它们要求将资产转移到托管代理，直到发生确定新所有者的事件为止。

- 去中心化交易所，可在匹配的买价和卖价之间转移资产。

- 将资产转移给获胜者的拍卖。

- 收集奖品并将其重新分配给获奖者的游戏或预测市场。

程序派生地址：

1. 允许程序控制特定的地址（称为程序地址），以使任何外部用户都无法生成带有这些地址签名的有效交易。

2. 允许程序以编程方式签名通过[跨程序调用](#cross-program-invocations)调用的指令中存在的程序地址。

在这两个条件下，用户可以安全地将链上资产的权限转移或分配给程序地址，然后程序可以自行决定在其他地方分配该权限。

### 程序地址的私钥

程序地址不在ed25519曲线上，因此没有与之关联的有效私钥，因此无法生成签名。  虽然它没有自己的私钥，但是程序可以使用它来发布包含程序地址作为签名者的指令。

### 基于哈希的生成程序地址

程序地址是使用256位抗映像前哈希函数从种子和程序ID的集合中确定性地得出的。  程序地址一定不能位于ed25519曲线上，以确保没有关联的私钥。 如果发现地址位于曲线上，则在生成过程中将返回错误。  对于给定的种子和程序ID集合，这种情况大约发生50/50的变化。  如果发生这种情况，可以使用另一组种子或种子凹凸(附加的8位种子) 来查找曲线外的有效程序地址。

程序的确定性程序地址遵循与使用`system_instruction::create_address_with_seed`实现的用`SystemInstruction::CreateAccountWithSeed`创建的帐户类似的派生路径。

作为参考，该实现如下：

```rust,ignore
pub fn create_address_with_seed(
    base: &Pubkey,
    seed: &str,
    program_id: &Pubkey,
) -> Result<Pubkey, SystemError> {
    if seed.len() > MAX_ADDRESS_SEED_LEN {
        return Err(SystemError::MaxSeedLengthExceeded);
    }

    Ok(Pubkey::new(
        hashv(&[base.as_ref(), seed.as_ref(), program_id.as_ref()]).as_ref(),
    ))
}
```

程序可以使用种子确定性地派生任意数量的地址。 这些种子可以象征性地标识地址的使用方式。

来自`Pubkey`::

```rust,ignore
///生成派生程序地址
/// *种子，用于派生密钥的符号关键字
/// * program_id，为该地址派生的程序
pub fn create_program_address(
    seeds: &[&[u8]],
    program_id: &Pubkey,
) -> Result<Pubkey, PubkeyError>
```

### 使用程序地址

客户可以使用`create_program_address`函数生成目标地址。

```rust,ignore
//确定性地导出托管密钥
let escrow_pubkey = create_program_address(&[&["escrow"]], &escrow_program_id);

//使用该密钥构造传输消息
let message = Message::new(vec![
    token_instruction::transfer(&alice_pubkey, &escrow_pubkey, 1),
]);

//处理将一个1令牌传输到托管的消息
client.send_and_confirm_message(&[&alice_keypair], &message);
```

程序可以使用相同的函数来生成相同的地址。 程序在下面的功能中从程序地址发出`token_instruction::transfer`，就好像它具有用于签署交易的私钥一样。

```rust,ignore
fn transfer_one_token_from_escrow(
    program_id: &Pubkey,
    keyed_accounts: &[KeyedAccount]
) -> Result<()> {

    //用户提供目的地
    let alice_pubkey = keyed_accounts[1].unsigned_key();

    //确定性派生托管公钥。
    let escrow_pubkey = create_program_address(&[&["escrow"]], program_id);

    //创建转移指令
let instruction = token_instruction::transfer(&escrow_pubkey, &alice_pubkey, 1);

    //运行时确定性地从当前
    //执行程序ID和提供的关键字。
    //如果派生地址与指令中标记为已签名的键匹配
    //然后该密钥被接受为已签名。
    invoke_signed(&instruction,  &[&["escrow"]])?
}
```

### 需要签名者的说明

用`create_program_address`生成的地址与任何其他公钥都没有区别。 运行时验证地址是否属于程序的唯一方法是使程序提供用于生成地址的种子。

运行时将在内部调用`create_program_address`，并将结果与指令中提供的地址进行比较。

## 示例

请参阅 [使用Rust开发](developing/on-chain-programs/developing-rust.md#examples) 和 [使用C开发](developing/on-chain-programs/developing-c.md#examples)以获取有关如何使用跨程序调用的示例。
