# 长期RPC事务历史
RPC需要提供至少6个月的交易历史。  当前的历史记录，以天为单位，对于下游用户来说是不够的。

6个月的交易数据实际上无法存储在验证节点的rocksdb账本中，所以需要一个外部数据存储。   验证节点的rocksdb账本将继续作为主要数据源，然后将回落到外部数据存储中。

受影响的RPC端点是： * [getFirstAvailableBlog]。
* [getFirstAvailableBlock](developing/clients/jsonrpc-api.md#getfirstavailableblock)
* [getConfirmedBlock](developing/clients/jsonrpc-api.md#getconfirmedblock)
* [getConfirmedBlocks](developing/clients/jsonrpc-api.md#getconfirmedblocks)
* [getConfirmedSignaturesForAddress](developing/clients/jsonrpc-api.md#getconfirmedsignaturesforaddress)
* [getConfirmedTransaction](developing/clients/jsonrpc-api.md#getconfirmedtransaction)
* [getSignatureStatuses](developing/clients/jsonrpc-api.md#getsignaturestatuses)

需要注意的是，不支持[getBlockTime](developing/clients/jsonrpc-api.md#getblocktime)，因为一旦https://github.com/Solana-labs/Solana/issues/10089 被修复，那么`getBlockTime`就可以被删除。

一些系统设计限制。
* 需要存储和搜索的数据量可以快速的跳到TB级，并且是不可改变的。
* 系统应该尽可能的轻量化，以满足SRE的要求。  例如一个SQL数据库集群，需要SRE不断地监控和重新平衡节点是不可取的。
* 数据必须可以实时搜索--花几分钟或几小时运行的批量查询是不可接受的。
* 易于在全球范围内复制数据，以便与将利用数据的RPC端点共同定位。
* 与外部数据存储的接口应该是容易的，不需要依赖风险较小的社区支持的代码库。

基于这些约束条件，选择Google的BigTable产品作为数据存储。

## 表模式
一个BigTable实例用来保存所有的交易数据，分成不同的表，以便快速搜索。

新数据可以随时复制到实例中，而不影响现有数据，所有数据都是不可改变的。  一般情况下，人们期望当前一个纪元完成后就会上传新数据，但对数据转储的频率没有限制。

通过适当配置实例表的数据保留策略，旧数据的清理是自动的，只是消失了。  因此数据添加的时间顺序就变得很重要。  例如如果在N-1纪元的数据之后添加了N纪元的数据，那么旧纪元的数据就会比新数据的寿命更长。  然而除了在查询结果中产生_holes_之外，这种无序删除不会有任何不良影响。  请注意，这种清理方法有效地允许存储无限量的事务数据，只是受限于这样做的货币成本。

表布局s只支持现有的RPC端点。  未来新的RPC端点可能需要对模式进行添加，并有可能对所有事务进行迭代以建立必要的元数据。

## 访问BigTable
BigTable有一个gRPC端点，可以使用[tonic](https://crates.io/crates/crate)] 和原始protobuf API进行访问，因为目前还没有针对BigTable的更高级别的Rust crate存在。  实际上，这使得BigTable查询结果的解析变得更加复杂，但并不是一个重要的问题。

## 数据群
通过使用新的`solana-ledger-tool`命令，将给定插槽范围的rocksdb数据转换为实例模式，实例数据的持续填充将以一个纪元的节奏进行。

同样的过程将被手动运行一次，以回填现有的账本数据。

### 区块表格：`block`

此表包含了给定插槽的压缩块数据。

行键是通过取插槽的16位小写十六进制表示来生成的，以确保当行被列出时，具有已确认块的最老的插槽总是排在第一位。  例如，插槽42的行键是00000000000000002a。

行数据是一个压缩的`StoredConfirmedBlock`结构。


### 账户地址交易签名查询表: `tx-by-addr`

该表包含了影响给定地址的事务。

行的键是`<base58
address>/<slot-id-one's-compliment-hex-slot-0-prefixed-to-16-digits>`。  行数据是一个压缩的`TransactionByAddrInfo`结构。

取插槽的一的补码允许列出插槽，确保最新的插槽与影响地址的事务总是会先列出。

Sysvar地址是没有索引的。  然而，经常使用的程序，如 Vote 或 System 是有索引的，并且很可能为每个确认的插槽有一行。

### 事务签名查询表: `tx`

该表将交易签名映射到其确认的区块，以及该区块中的索引。

行键是base58编码的交易签名。 行数据是一个压缩的`TransactionInfo`结构。
