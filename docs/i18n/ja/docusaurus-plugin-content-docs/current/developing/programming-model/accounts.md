---
title: "アカウント"
---

## トランザクション間での状態の保存

プログラムがトランザクション間の状態を保存する必要がある場合は、*アカウント*を使って保存します。 アカウントは、Linux などのオペレーティング・システムにおけるファイルに似ています。 ファイルと同様に、アカウントは任意のデータを保持することができ、そのデータはプログラムの寿命を超えて持続します。 また、ファイルと同様に、アカウントにはメタデータが含まれており、ランタイムは誰がどのようにデータにアクセスすることを許可されているかを知ることができます。

ファイルとは異なり、アカウントにはファイルのライフタイムのメタデータが含まれています。 その寿命は「トークン」という単位で表され、これは*ランポート*と呼ばれる小数のネイティブトークンの数です。 アカウントはバリデータのメモリに保持され、そこに留まるために["家賃"](#rent) を支払います。 各バリデータは定期的にすべてのアカウントをスキャンし、賃料を徴収します。 ランポートが 0 になったアカウントはパージされます。 Accounts can also be marked [rent-exempt](#rent-exemption) if they contain a sufficient number of lamports.

Linux ユーザーがパスを使ってファイルを検索するのと同じように、Solana クライアントは*アドレス*を使ってアカウントを検索します。 このアドレスは、256 ビットの公開キーです。

## 署名者

トランザクションには、そのトランザクションで参照されるアカウントの公開キーに対応するデジタル[署名](terminology.md#signature)が含まれることがあります。 対応するデジタル署名が存在する場合、そのアカウントの秘密キーの保有者が署名し、その結果、取引を"承認"したことを意味し、そのアカウントは*署名者*と呼ばれます。 あるアカウントが署名者であるかどうかは、アカウントのメタデータの一部としてプログラムに伝えられます。 プログラムは、その情報を使って権限の決定を行うことができる。

## 読み取り専用

Transactions can [indicate](transactions.md#message-header-format) that some of the accounts it references be treated as _read-only accounts_ in order to enable parallel account processing between transactions ランタイムは、複数のプログラムによる読み取り専用アカウントの同時読み取りを許可します。 プログラムが読み取り専用アカウントを修正しようとすると、そのトランザクションはランタイムによって拒否されます。

## 実行可能

アカウントがメタデータ内で「executable」とマークされている場合、アカウントの公開キーを 命令の プログラム ID [に含めることによって実行される](transactions.md#program-id)プログラムと見なされます。 アカウントは、そのアカウントを所有するローダがプログラムのデプロイメントプロセスに成功したときに、実行可能とマークされます。 例えば、BPF プログラムのデプロイ時に、ローダーがアカウントのデータに含まれる BPF バイトコードが有効であると判断すると、ローダーはプログラムアカウントを実行可能なものとして永続的にマークします。 実行可能になると、 ランタイムは、アカウントのデータ (プログラム) が変更不能であることを強制します。

## 作成中

アカウントを作成するには、クライアントが _キーペア_ を生成し、 公開キーを `SystemProgram::CreateAccount` 命令を使用して バイト単位の固定ストレージサイズを事前に割り当てます。 現在、アカウントのデータの最大サイズは 10 メガバイトです。

アカウント・アドレスは任意の 256 ビットの値を取ることができますが、上級ユーザが派生したアドレスを作成するためのメカニズムがあります。 (`SystemProgram::CreateAccountWithSeed`, [`Pubkey::CreateProgramAddress`](calling-between-programs.md#program-derived-addresses)).があります。

システムプログラムで作成されたことのないアカウントもプログラムに渡すことができます。 以前に作成されたことのないアカウントを参照する命令の場合、プログラムには、システムプログラムが所有し、ランポートがゼロで、データがゼロのアカウントが渡されます。 しかし、このアカウントは、トランザクションの署名者であるかどうかを反映するので、権限として使用することができます。 ここでいう権限とは、アカウントの公開キーに関連付けられた秘密キーの保有者がトランザクションに署名したことをプログラムに伝えるものです。 アカウントの公開キーは、プログラムが知っているものでも、別のアカウントに記録されているものでもよく、プログラムが制御または実行する資産や操作に対する何らかの所有権や権限を意味します

## プログラムのオーナーシップと割り当て

作成されたアカウントは、システムプログラムと呼ばれる組み込みプログラムが*所有する*ように初期化され、それを適確に*システムアカウント*と呼びます。 アカウントには「オーナー」のメタデータが含まれています。 所有者はプログラム id です。 ランタイムは、プログラムの id が所有者と一致した場合に、そのアカウントへの書き込みアクセスを許可します。 システムプログラムの場合、ランタイムはクライアントがランポートを転送することを可能にし、重要なことに \_\_ アカウントの所有権を割り当てます。 所有者を別のプログラム id に変更することです アカウントがプログラムによって所有されていない場合、プログラムはそのデータの読み取りとアカウントのクレジットのみが許可されます。

## Verifying validity of unmodified, reference-only accounts

For security purposes, it is recommended that programs check the validity of any account it reads but does not modify.

The security model enforces that an account's data can only be modified by the account's `Owner` program. Doing so allows the program to trust that the data passed to them via accounts they own will be in a known and valid state. The runtime enforces this by rejecting any transaction containing a program that attempts to write to an account it does not own. But, there are also cases where a program may merely read an account they think they own and assume the data has only been written by themselves and thus is valid. But anyone can issues instructions to a program, and the runtime does not know that those accounts are expected to be owned by the program. Therefore a malicious user could create accounts with arbitrary data and then pass these accounts to the program in the place of a valid account. The arbitrary data could be crafted in a way that leads to unexpected or harmful program behavior.

To check an account's validity, the program should either check the account's address against a known value or check that the account is indeed owned correctly (usually owned by the program itself).

One example is when programs read a sysvar. Unless the program checks the address or owner, it's impossible to be sure whether it's a real and valid sysvar merely by successful deserialization. Accordingly, the Solana SDK [checks the sysvar's validity during deserialization](https://github.com/solana-labs/solana/blob/a95675a7ce1651f7b59443eb146b356bc4b3f374/sdk/program/src/sysvar/mod.rs#L65).

If the program always modifies the account in question, the address/owner check isn't required because modifying an unowned (could be the malicious account with the wrong owner) will be rejected by the runtime, and the containing transaction will be thrown out.

## Rent

Solana でアカウントを存続させるには、クラスターがデータを積極的に維持して、将来のトランザクションを処理する必要があるため、*賃貸料*というストレージコストが発生します。 これは、ビットコインやイーサリアムのようにアカウントを保存することにコストがかからない場合とは異なります。

使用料は、現在のエポックにおける最初のアクセス(最初のアカウント作成を含む。) 時に、トランザクションによってランタイムによってアカウントの残高から引き落とされ、トランザクションがない場合は 1 エポックにつき 1 回引き落とされる。 料金は現在、固定料金で、単位はバイト・タイム・エポックです。 料金は将来的に変更される可能性があります。

家賃の計算を簡単にするために、家賃は常に 1 エポックで徴収されます。家賃は日割り計算されません。 つまり、部分的なエポックのための手数料や返金はありません。つまり、アカウント作成時に最初に徴収される家賃は、現在の部分的なエポックのためではなく、次の完全なエポックのために前もって徴収されるということです。 その後の家賃の徴収は、さらに将来のエポックに対して行われます。 その後の家賃の徴収は、さらに将来のエポックに対して行われます。 一方で、すでに家賃を徴収したアカウントの残高がエポックの途中で別の家賃を下回った場合、そのアカウントは現在のエポックまで存在し続け、次のエポックの開始時に直ちに消去されます。

最低限の残高を維持している口座は、家賃の支払いが免除されます。 この家賃免除については以下の通りです。

### 家賃の計算

注意: 家賃は将来的に変更される可能性があります。

執筆時点で、固定料金は testnet および mainnet-beta クラスタの byte-エポック あたりの 19.055441478439427 ランポートです。 [ エポック](terminology.md#epoch) は 2 日を目安にしています(devnet の場合、54m36s のエポックで 1 バイトエポックあたり 0.3608183131797095 ランポートのレンタル料です)。

この値は、0.01SOL/mebibyte-day(3.56SOL/mebibyte-year にぴったり一致) を目標に計算されています。

```text
料金: 19.055441478439427 = 10_000_000 (0.01 SOL) ※365(約1年後) / (1024 * 1024)(1 MiB) / (365.25/2)(1年後のエポック)
```

また，家賃の計算は`f64`の精度で行われ，最終結果はランポートの`u64`に切り捨てられます。

賃貸料計算には、アカウントのサイズのアカウントメタデータ (住所、所有者、ランポートなど) が含まれます。 そのため、家賃計算のためのアカウントの最小サイズは 128 バイトとなります。

例えば、最初に 10,000 ランポートを送金し、追加データがない状態でアカウントを作成します。 作成と同時に家賃が引き落とされ、残高は 7,561 ランポートとなります。

```text
家賃: 2,439 = 19。 55441478439427 (賃貸料) * 128 バイト (最小口座サイズ) * 1 (エポック)
口座残高: 7, 61 = 10,000(転送されたランポート)- 2,439(このアカウントのエポック料金)
```

アカウント残高は、活動がなくても次のエポックで 5,122lamports に減少します。

```text
口座残高: 5,122 = 7,561(現在の残高) - 2,439 (この口座のエポック料金)
```

したがって、最小サイズのアカウントは、作成後、転送されたランポートが 2,439 以下であれば、すぐに削除されます。

### 家賃の免除設定

また、最低でも 2 年分の家賃を預けることで、家賃の徴収を完全に免除することもできます。これはアカウントの残高が減るたびにチェックされ、残高が最低額以下になると家賃が即座に引き落とされるます。 プログラム実行可能なアカウントは、パージされないように家賃免除されることがランタイムによって要求されます。

プログラム実行可能なアカウントは、パージされないようにするために、ランタイムでは家賃免除が要求されます。

注: [`getMinimumBalanceForRentExemption` RPC エンドポイント](developing/clients/jsonrpc-api.md#getminimumbalanceforrentexemption) を使用して、特定の口座サイズの最小残高を計算します。 以下の計算は例示です。

例えば、15,000 バイトのサイズのプログラム実行可能プログラムでは、 賃貸料免除を必要とする 105,290,880 lamports(=~ 0.05 SOL) の残高が必要です。

```text
105,290,880 = 19.055441478439427 (fee rate) * (128 + 15_000)(account size including metadata) * ((365.25/2) * 2)(epochs in 2 years)
```

Rent can also be estimated via the [`solana rent` CLI subcommand](cli/usage.md#solana-rent)

```text
$ solana rent 15000
Rent per byte-year: 0.00000348 SOL
Rent per epoch: 0.000288276 SOL
Rent-exempt minimum: 0.10529088 SOL
```

Note: Rest assured that, should the storage rent rate need to be increased at some point in the future, steps will be taken to ensure that accounts that are rent-exempt before the increase will remain rent-exempt afterwards
