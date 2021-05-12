---
title: "账户"
---

## 在交易之间存储状态

如果程序需要在交易之间存储状态，则可以使用*accounts*进行存储。 帐户类似于 Linux 等操作系统中的文件。 就像文件一样，帐户可以保存任意数据，并且该数据会在程序的生存期内持续存在。 帐户也像文件一样，包含元数据，该元数据告诉运行时允许谁访问数据以及如何访问数据。

与文件不同，该帐户包含文件生存期内的元数据。 该存在时间用“代币”表示，即称为*lamports*的许多局部原生代币。 帐户保存在验证节点的内存中，并支付[“rent”](#rent)留在那里。 每个验证节点都会定期扫描所有帐户并收取租金。 任何掉落到零零花的账户都将被清除。 Accounts can also be marked [rent-exempt](#rent-exemption) if they contain a sufficient number of lamports.

与 Linux 用户使用路径查找文件的方式相同，Solana 客户端使用*address*查找帐户。 该地址是一个 256 位公共密钥。

## 签名者

交易可以包括与交易所引用的账户的公共密钥相对应的数字[签名](terminology.md#signature)。 当存在相应的数字签名时，它表示该帐户的私钥持有人已签名并因此“授权”了该交易，因此该帐户称为*signer*。 帐户是否为签名者将作为帐户元数据的一部分传达给程序。 然后，程序可以使用该信息来制定权限决策。

## 只读

事务可以[指示](transactions.md#message-header-format)它引用的某些帐户被视为*只读帐户*，以便能够在事务之间进行并行帐户处理。 运行时允许多个程序同时读取只读帐户。 如果程序尝试修改只读帐户，则运行时将拒绝该事务。

## 可执行

如果某个帐户在其元数据中被标记为“可执行”，则将其视为可以通过将帐户的公钥包含在指令的[程序 ID](transactions.md#program-id)中来执行的程序。 在成功的程序部署过程中，拥有该帐户的加载程序将帐户标记为可执行文件。 例如，在 BPF 程序部署期间，一旦加载程序确定帐户数据中的 BPF 字节码有效，则加载程序会将程序帐户永久标记为可执行文件。 一旦可执行，运行时将强制该帐户的数据(程序) 是不可变的。

## 创建

为了创建一个帐户，客户端生成一个*keypair*并使用`SystemProgram::CreateAccount`指令注册其公共密钥，并预先分配了固定的存储大小（以字节为单位）。 当前帐户数据的最大大小为 10MB。

帐户地址可以是任意的 256 位值，并且高级用户可以使用一些机制来创建派生地址(`SystemProgram::CreateAccountWithSeed`，[`Pubkey::CreateProgramAddress`](calling-between-programs.md#program-derived-addresses))。

从未通过系统程序创建的帐户也可以传递到程序。 当指令引用以前尚未创建的帐户时，程序将通过系统程序拥有的帐户，该帐户具有 0 个 Lamport 和 0 个数据。 但是，该帐户将反映它是否是该交易的签名者，因此可以用作授权。 在这种情况下，授权机构向程序传达与帐户的公共密钥相关联的私有密钥的持有者对交易进行了签名。 该程序可能知道该帐户的公钥，也可能将其记录在另一个帐户中，并表示对该程序控制或执行的资产或操作具有某种所有权或授权。

## 程序的所有权和分配

创建的帐户由称为 System 程序的内置程序初始化为*owned*，并适当地称为*system account*。 帐户包含“所有者”元数据。 所有者是一个程序 ID。 如果运行时的 ID 与所有者匹配，则运行时将授予该程序对该帐户的写访问权限。 对于 System 程序，运行时允许客户端转移 Lamport，并且重要的是*转移*帐户所有权，这意味着将所有者更改为其他程序 ID。 如果某个帐户不属于某个程序，则仅允许该程序读取其数据并将该帐户记入贷方。

## Verifying validity of unmodified, reference-only accounts

For security purposes, it is recommended that programs check the validity of any account it reads but does not modify.

The security model enforces that an account's data can only be modified by the account's `Owner` program. Doing so allows the program to trust that the data passed to them via accounts they own will be in a known and valid state. The runtime enforces this by rejecting any transaction containing a program that attempts to write to an account it does not own. But, there are also cases where a program may merely read an account they think they own and assume the data has only been written by themselves and thus is valid. But anyone can issues instructions to a program, and the runtime does not know that those accounts are expected to be owned by the program. Therefore a malicious user could create accounts with arbitrary data and then pass these accounts to the program in the place of a valid account. The arbitrary data could be crafted in a way that leads to unexpected or harmful program behavior.

To check an account's validity, the program should either check the account's address against a known value or check that the account is indeed owned correctly (usually owned by the program itself).

One example is when programs read a sysvar. Unless the program checks the address or owner, it's impossible to be sure whether it's a real and valid sysvar merely by successful deserialization. Accordingly, the Solana SDK [checks the sysvar's validity during deserialization](https://github.com/solana-labs/solana/blob/a95675a7ce1651f7b59443eb146b356bc4b3f374/sdk/program/src/sysvar/mod.rs#L65).

If the program always modifies the account in question, the address/owner check isn't required because modifying an unowned (could be the malicious account with the wrong owner) will be rejected by the runtime, and the containing transaction will be thrown out.

## 出租

使帐户在 Solana 上保持活动状态会产生称为*rent*的存储成本，因为集群必须积极维护数据以处理其上的任何将来的事务。 这与比特币和以太坊不同，在比特币和以太坊中，存储帐户不会产生任何费用。

租金是在当前时期通过事务在第一次访问(包括初始帐户创建) 时通过运行时从帐户余额中扣除的，如果没有交易，则在每个时期一次。 该费用目前是固定费率，以字节乘以时期为单位。 该费用将来可能会更改。

为了简化租金计算，租金始终是在一个完整的时期内收取的。 租金不是按比例分配的，这意味着部分时期既不收费也不退款。 这意味着，在创建帐户时，收取的首笔租金不是针对当前的部分时期，而是针对下一个完整的时期而预先收取的租金。 随后的租金收取是未来的进一步时期。 另一方面，如果一个已出租的帐户的余额降到另一个租金费用的中间时期以下，则该帐户将在当前时期继续存在，并在即将到来的时期开始时立即被清除。

如果帐户保持最低余额，则可以免交租金。 此免租金描述如下。

### 租金计算

注意：租金率将来可能会改变。

在撰写本文时，在 testnet 和 mainnet-beta 群集上，固定租金为每字节纪元 19.055441478439427 兰特。 一个[epoch](terminology.md#epoch)的目标是 2 天(对于 devnet，租金为每字节纪元 0.3608183131797095 lamports，长度为 54m36s 长)。

计算得出该值的目标是每兆字节天 0.01 SOL(与每兆字节年 3.56SOL 完全匹配)：

```text
租金：19.055441478439427=10_000_000(0.01SOL)*365(一年中大约一天)/(1024*1024)(1MiB)/(365.25/2)(一年中的纪元)
```

租金计算以`f64`精度完成，最终结果在 Lamports 中被截断为`u64`。

租金计算包括帐户大小的帐户元数据(地址、所有者、lamports 等)。 因此，用于租金计算的最小帐户为 128 字节。

例如，创建的帐户初始转移了 10,000 lamports，并且没有其他数据。 租金会在创建时立即从中扣除，从而产生 7,561 lamports 的余款：

```text
租金：2,439=19.055441478439427(租金)*128字节(最小帐户大小)*1(纪元)
帐户余额：7,561=10,000(转让的兰特)-2,439(此帐户的时期租金)
```

即使没有活动，帐户余额也将在下一个时期减少到 5,122 lamports：

```text
帐户余额：5,122=7,561(当前余额) -2,439(该帐户的租金，用于某个时期)
```

因此，如果转移的兰特小于或等于 2439，则最小尺寸帐户将在创建后立即删除。

### 免租金

另外，通过存入至少 2 年的租金，可以使一个帐户完全免收租金。 每次帐户余额减少时都会进行检查，一旦余额低于最低金额，便会立即从租金中扣除。

运行时要求程序可执行帐户免租金，以免被清除。

注意：请使用[`getMinimumBalanceForRentExemption`RPC 端点](developing/clients/jsonrpc-api.md#getminimumbalanceforrentexemption)计算特定帐户大小的最小余额。 以下计算仅是说明性的。

例如，一个程序可执行文件的大小为 15,000 字节，则需要 105,290,880 lamports(=〜0.105SOL) 的余额才能免租：

```text
105,290,880=19.055441478439427(手续费率)*(128+15_000)(包括元数据的帐户大小)*((365.25/2)*2)(以2年为周期)
```

Rent can also be estimated via the [`solana rent` CLI subcommand](cli/usage.md#solana-rent)

```text
$ solana rent 15000
Rent per byte-year: 0.00000348 SOL
Rent per epoch: 0.000288276 SOL
Rent-exempt minimum: 0.10529088 SOL
```

Note: Rest assured that, should the storage rent rate need to be increased at some point in the future, steps will be taken to ensure that accounts that are rent-exempt before the increase will remain rent-exempt afterwards
