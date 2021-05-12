---
title: Структура Stake аккаунта
---

Stake аккаунт в Solana может быть использован для делегирования токенов валидаторам в сети, чтобы потенциально заработать вознаграждение владельцу stake аккаунта. Stake accounts are created and managed differently than a traditional wallet address, known as a _system account_. Системная учетная запись может отправлять и получать SOL только от других учетных записей в сети, в то время как stake аккаунт поддерживает более сложные операции, необходимые для управления делегированием токенов.

Stake аккаунты в Solana работают иначе, чем у других сетей Proof-of-Stake сетях, с которыми вы можете быть знакомы. Этот документ описывает высокоуровневую структуру и функции stake аккаунтов Solana.

#### Адрес аккаунта

Каждая учетная запись имеет уникальный адрес, который может быть использован для поиска учетной записи в командной строке или в любых инструментах сетевого обозревателя. Однако, в отличие от адреса кошелька, в котором владелец ключа управляет кошельком, ключ, связанный с адресом stake аккаунта, не обязательно имеет контроль над аккаунтом. Фактически, сам ключ или приватный ключ могут даже не существовать для адреса stake аккаунта.

Единственным случаем, когда адрес stake аккаунта имеет файл ключей является [создание stake аккаунта с помощью инструментов командной строки](../cli/delegate-stake.md#create-a-stake-account), новый ключ создается только для того, чтобы удостовериться в том, что адрес stake аккаунта является новым и уникальным.

#### Описание Account Authorities

Certain types of accounts may have one or more _signing authorities_ associated with a given account. Account authority используется для подписания определенных транзакций для учетной записи, которую она контролирует. Это отличается от некоторых других блокчейн сетей, где владелец ключа, связанного с аккаунтом, контролирует все действия аккаунта.

В stake аккаунте есть два signing authorities, которые указаны их соответствующим адресом, каждый из которых имеет право выполнять определенные операции со stake аккаунтом.

The _stake authority_ is used to sign transactions for the following operations:

- Делегирование стейка
- Деактивация стейка
- Разделение stake аккаунта и создание нового, с использованием части средств от первого аккаунта
- Merging two stake accounts into one
- Установка новых stake authority

The _withdraw authority_ signs transactions for the following:

- Вывод неделегированного стейка на адрес кошелька
- Установка новых withdraw authority
- Установка новых stake authority

Stake authority and withdraw authority устанавливаются при создании stake аккаунта и могут быть изменены для авторизации нового подписывающего адреса в любое время. Stake and withdraw authority может быть одним и тем же адресом или двумя разными.

Ключ withdraw authority имеет больший контроль над аккаунтом, поскольку он необходим для ликвидации токенов на stake аккаунте и может быть использован для сброса полномочий stake authority, если пара ключей stake authority была потеряна или скомпрометирована.

Защита withdraw authority от потери или кражи крайне важна при управлении stake аккаунтом.

#### Множественные делегации

Каждый stake аккаунт может быть использован только для делегирования одному валидатору. Все токены аккаунта могут быть либо делегированными, либо не делегированными. Или же находиться в процессе делегирования или разделегирования. Чтобы делегировать часть ваших токенов валидатору или делегировать нескольким валидаторам - вы должны создать несколько аккаунтов.

Это может быть достигнуто путем создания нескольких stake аккаунтов или путем создания одного крупного аккаунта и использования stake authority для разделения счета на несколько счетов с остатками токенов по вашему выбору.

Одни и те же полномочия по стейку и withdraw authorities могут быть назначены нескольким stake аккаунтам.

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
