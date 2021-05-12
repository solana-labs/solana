---
title: ステーキングアカウントの構成
---

Solana のステーキングアカウントは、ネットワーク上のトークンをバリデータにデリゲートし、ステーキングアカウントの所有者に対して報酬を得る可能性があります。 Stake accounts are created and managed differently than a traditional wallet address, known as a _system account_. システムアカウントは、ネットワーク上の他のアカウントと SOL を送受信することしかできませんが、ステーキングアカウントは、トークンのデリゲーションを管理するために必要な、より複雑な操作をサポートします。

Solana のステーキングアカウントも、あなたがよく知っているかもしれない他の PoS ブロックチェーンネットワークのアカウントとは異なる動作をします。 このドキュメントでは、Solana の投資アカウントの高レベルの構造と関数について説明します。

#### アカウントアドレス

各ステーキングアカウントには固有のアドレスがあり、これを使ってコマンドラインやネットワークエクスプローラーツールでアカウント情報を調べることができます。 しかし、ウォレットのアドレスでは、そのアドレスのキーペアの保有者がウォレットを管理していますが、ステーキングアカウントのアドレスに関連するキーペアは、必ずしもアカウントを管理しているわけではありません。 実際には、ステーキングアカウントのアドレスに、キーペアや秘密キーが存在しない場合もあります。

ステーキングアカウントのアドレスにキーペアファイルがあるのは、[コマンドラインツールを使ってステーキングアカウントを作成するときだけです](../cli/delegate-stake.md#create-a-stake-account)。新しいキーペアファイルが最初に作成されるのは、ステーキングアカウントのアドレスが新しくてユニークであることを確認するためだけです。

#### アカウントの権限を理解しよう

Certain types of accounts may have one or more _signing authorities_ associated with a given account. アカウント権限は、それが管理するアカウントの特定のトランザクションに署名するために使用されます。 これは、アカウントのアドレスに関連付けられたキーペアの保有者がアカウントのすべての活動を制御する他のいくつかのブロックチェーンネットワークとは異なります。

各ステーキングアカウントには、それぞれのアドレスで指定された 2 つの署名権限があり、それぞれがステーキングアカウントに対して特定の操作を行う権限を持っています。

The _stake authority_ is used to sign transactions for the following operations:

- ステーキングのデリゲーション
- ステーキングデリゲーションの無効化
- ステーキングアカウントを分割し、最初のアカウントの資金の一部を使って新しいステーキングアカウントを作成すること
- Merging two stake accounts into one
- 新しいステーキングの権限の設定

The _withdraw authority_ signs transactions for the following:

- 委任されていないステーキングをウォレットアドレスに出金すること
- 新しい引き出し権限の設定
- 新しいステーキング権限の設定

ステーキング権限と引き出し権限は、ステーキングアカウントが作成されたときに設定され、いつでも新しい署名アドレスを認証するために変更することができます。 ステーキング権限と引き出し権限は、同じアドレスでも 2 つの異なるアドレスでも構いません。

引出権者のキーペアは、ステーキングアカウント内のトークンを清算するために必要とされるため、アカウントに対するより多くのコントロールを保持し、ステーキング権限者のキーペアが紛失または侵害された場合には、ステーキング権限者をリセットするために使用することができます。

ステーキングアカウントを管理する際には、引き出し権限を紛失や盗難から守ることが何よりも重要です。

#### 複数のデリゲーション

各ステーキングアカウントは、一度に 1 人のバリデータに委任するためにのみ使用することができます。 アカウント内のすべてのトークンは、"委任されているか"、"委任されていないか"、"あるいは委任されているか・あるいは委任されていない状態になる過程"にあります。 トークンの一部をバリデータに委任する場合や、複数のバリデータに委任する場合は、複数のステーキングアカウントを作成する必要があります。

これは、いくつかのトークンを含むウォレットアドレスから複数のステーキングアカウントを作成したり、1 つの大きなステーキングアカウントを作成し、ステーキングオーソリティを使用してアカウントを任意のトークン残高を持つ複数のアカウントに分割したりすることで実現できます。

同じステーキングアカウントおよび引出権限を複数のステーキングアカウントに割り当てることができます。

#### Merging stake accounts

Two stake accounts that have the same authorities and lockup can be merged into a single resulting stake account. A merge is possible between two stakes in the following states with no additional conditions:

- two deactivated stakes
- an inactive stake into an activating stake during its activation epoch

For the following cases, the voter pubkey and vote credits observed must match:

- two activated stakes
- two activating accounts that share an activation epoch, during the activation epoch

All other combinations of stake states will fail to merge, including all "transient" states, where a stake is activating or deactivating with a non-zero effective stake.

#### Delegation Warmup and Cooldown

When a stake account is delegated, or a delegation is deactivated, the operation does not take effect immediately.

A delegation or deactivation takes several [epochs](../terminology.md#epoch) to complete, with a fraction of the delegation becoming active or inactive at each epoch boundary after the transaction containing the instructions has been submitted to the cluster.

There is also a limit on how much total stake can become delegated or deactivated in a single epoch, to prevent large sudden changes in stake across the network as a whole. Since warmup and cooldown are dependent on the behavior of other network participants, their exact duration is difficult to predict. Details on the warmup and cooldown timing can be found [here](../cluster/stake-delegation-and-rewards.md#stake-warmup-cooldown-withdrawal).

#### Lockups

Stake accounts can have a lockup which prevents the tokens they hold from being withdrawn before a particular date or epoch has been reached. While locked up, the stake account can still be delegated, un-delegated, or split, and its stake and withdraw authorities can be changed as normal. Only withdrawal into a wallet address is not allowed.

A lockup can only be added when a stake account is first created, but it can be modified later, by the _lockup authority_ or _custodian_, the address of which is also set when the account is created.

#### Destroying a Stake Account

Like other types of accounts on the Solana network, a stake account that has a balance of 0 SOL is no longer tracked. If a stake account is not delegated and all of the tokens it contains are withdrawn to a wallet address, the account at that address is effectively destroyed, and will need to be manually re-created for the address to be used again.

#### Viewing Stake Accounts

Stake account details can be viewed on the Solana Explorer by copying and pasting an account address into the search bar.

- http://explorer.solana.com/accounts
