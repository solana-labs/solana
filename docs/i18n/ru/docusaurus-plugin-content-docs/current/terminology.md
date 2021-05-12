---
title: Терминология
---

Следующие термины используются во всей документации.

## аккаунт

Постоянный файл, адресованный [публичным ключом](terminology.md#public-key) и [Лэмпорт](terminology.md#lamport) отслеживает его срок службы.

## приложение

Приложение для фронт-энда, которое взаимодействует с кластером Solana.

## статус банка

Результат интерпретации всех программ в реестре на заданной [tick height](terminology.md#tick-height). Он включает по крайней мере набор всех [аккаунтов](terminology.md#account) с нулевыми [родными токенами](terminology.md#native-tokens).

## блок

Непрерывный набор [entries](terminology.md#entry) в реестре, охваченных голосованием [vote](terminology.md#ledger-vote). [Лидер](terminology.md#leader) производит не более одного блока на [слот](terminology.md#slot).

## блок-хэш

Стойкий к предыдущему образу [хэш](terminology.md#hash) [реестр](terminology.md#ledger) на заданной [высоте блока](terminology.md#block-height). Снято с последнего [id записи](terminology.md#entry-id) в ячейке

## высота блока

Число блоков [](terminology.md#block) под текущим блоком. Первый блок после [блока генезиса](terminology.md#genesis-block) имеет высоту один.

## bootstrap валидатор

Первый [валидатор](terminology.md#validator), создавший [block](terminology.md#block).

## CBC блок

Самый маленький чанк с зашифрованным шифрованием - зашифрованный сегмент реестра будет состоять из многих блоков CBC. `ledger_segment_size / cbc_block_size` если быть точным.

## клиент

[узел](terminology.md#node), который использует [кластер](terminology.md#cluster).

## кластер

Набор валидаторов [](terminology. md#validator) поддерживающих один [реестра ](terminology. md#ledger).

## время подтверждения

Продолжительность времени между [лидером](terminology.md#leader), создающим [запись тика](terminology.md#tick), и созданием [подтвержденного блока](terminology.md#confirmed-block).

## подтвержденный блок

Блок [](terminology.md#block), который получил [супербольшинство](terminology.md#supermajority) из [голосов](terminology.md#ledger-vote) с интерпретацией реестра, которая соответствует лидеру.

## контрольная плоскость

Gossip сеть, соединяющая все [узлы](terminology.md#node) [кластера](terminology.md#cluster).

## время ожидания

Некоторое количество [эпох](terminology.md#epoch) после того, как [стейк](terminology.md#stake) был деактивирован, пока он постепенно становится доступен для снятия. В течение этого периода ставка считается "дезактивированной". Больше информации о: [прогревать и перезарядить](implemented-proposals/staking-rewards.md#stake-warmup-cooldown-withdrawal)

## кредит

Смотрите [кредит на голосование](terminology.md#vote-credit).

## плоскость данных

Групповая сеть, используемая для эффективной валидации [записей](terminology.md#entry) и достижения консенсуса.

## дрон

off-chain служба, которая действует как хранитель приватного ключа пользователя. Обычно служит для проверки и подписания транзакций.

## запись

Запись в [реестр ](terminology.md#ledger) либо [флаг](terminology.md#tick), либо [запись транзакций](terminology.md#transactions-entry).

## id записи

Хэш [устойчивый к прообразу](terminology.md#hash) над финальным содержанием записи, который выступает в качестве [уникального идентификатора](terminology.md#entry) в мире. Хэш служит доказательством того, что:

- Запись создается по прошествии некоторого времени
- Указанные [транзакции](terminology.md#transaction) включены в запись
- Позиция записи по отношению к другим записям в [реестр ](terminology.md#ledger)

Смотрите [Proof of History](terminology.md#proof-of-history).

## эпоха

Время, т.е. количество [слотов](terminology.md#slot), для которого [график лидера](terminology.md#leader-schedule) является действительным.

## комиссионный счет

Комиссионный счет в транзакции оплачивается за счет включения транзакции в реестр. Это первый аккаунт в транзакции. Этот аккаунт должен быть объявлен в транзакции Запись/Чтение(доступен для записи), так как оплата за транзакцию уменьшает остаток на счете.

## финальный

Когда узлы, представляющие 2/3 доли [](terminology.md#stake), имеют общий [корневой блок](terminology.md#root).

## форк

[Реестр](terminology.md#ledger) произведен от общих записей, но затем ответвлен.

## блок генезиса

Первый блок [](terminology.md#block) в цепочке.

## генезис конфигурация

Конфигурационный файл, который подготавливает [реестр](terminology.md#ledger) для [генезисного блока](terminology.md#genesis-block).

## хэш

Цифровой отпечаток пальца последовательности байтов.

## инфляция

Увеличение количества токенов с течением времени, используемое для финансирования вознаграждения за подтверждение и финансирования дальнейшего развития Solana.

## инструкция

Наименьшая единица [программы](terminology.md#program), которую [клиент](terminology.md#client) может включить в [транзакцию](terminology.md#transaction).

## пара ключей

[публичный ключ](terminology.md#public-key) и соответствующий [приватный ключ](terminology.md#private-key).

## лэмпорт

Дробный [родной токен](terminology.md#native-token) со значением 0.000000001 [sol](terminology.md#sol).

## лидер

Роль валидатора [](terminology.md#validator), когда он добавляет [записи](terminology.md#entry) в [реестр](terminology.md#ledger).

## график лидера

A sequence of [validator](terminology.md#validator) [public keys](terminology.md#public-key) mapped to [slots](terminology.md#slot). Кластер использует график лидера, чтобы определить, какой валидатор является [лидером](terminology.md#leader) в любой момент времени.

## реестр

Список [записей,](terminology.md#entry) содержащих [транзакций](terminology.md#transaction) подписанных [клиентами](terminology.md#client). Концептуально это можно проследить до [блока генезиса](terminology.md#genesis-block), но в реестре [валидаторов](terminology.md#validator) могут быть только новые [блоки](terminology.md#block) для экономии использования хранилища, поскольку старые не нужны для проверки будущих блоков по дизайну.

## голос за реестр

[Хеш](terminology.md#hash) [состояния валидатора](terminology.md#bank-state) на заданной [высоте тика](terminology.md#tick-height). Он содержит подтверждение [валидатора](terminology.md#validator) о том, что полученный блок [](terminology.md#block) был проверен, а также обещание не голосовать за конфликтующий блок [](terminology.md#block) \(i.. [Форк](terminology.md#fork)\) на определенное количество времени, блокировка [на](terminology.md#lockout) период.

## лёгкий клиент

Тип [клиента](terminology.md#client), который может проверить правильность указания [кластера](terminology.md#cluster). Он выполняет больше проверок реестра, чем [тонкий клиент](terminology.md#thin-client), и меньше, чем [валидатор](terminology.md#validator).

## загрузчик

Программа [](terminology.md#program) с возможностью интерпретировать бинарную кодировку других on-chain программ.

## блокировка

Продолжительность времени, в течение которого [валидатор](terminology.md#validator) не может [проголосовать](terminology.md#ledger-vote) за другой [форк](terminology.md#fork).

## родной токен

[token](terminology.md#token) используется для отслеживания работы, выполненной [узлами](terminology.md#node) в кластере.

## узел

Компьютер, участвующий в [кластере](terminology.md#cluster).

## количество узлов

Число валидаторов [](terminology.md#validator) участвующих в [кластере](terminology.md#cluster).

## PoH

Смотрите [Proof of History](terminology.md#proof-of-history).

## точка

Взвешенный [кредит](terminology.md#credit) в режиме вознаграждения. В валидаторе [](terminology.md#validator) [режима вознаграждения](cluster/stake-delegation-and-rewards.md), количество баллов, причитающихся за ставку [](terminology.md#stake) во время погашения является произведением заработанных [баллов за голосования,](terminology.md#vote-credit) и количества поставленных лэмпортов.

## приватный ключ

Приватный ключ [пары ключей](terminology.md#keypair).

## программа

Код, который интерпретирует [инструкции](terminology.md#instruction).

## id программы

Публичный ключ аккаунта [](terminology.md#account), содержащей [программу](terminology.md#program).

## Proof of History

Стек доказательств, каждое из которых доказывает, что некоторые данные существовали до того, как доказательство было создано, и что точный промежуток времени прошел до предыдущего доказательства. Как и [VDF](terminology.md#verifiable-delay-function), Proof of History может быть проверено за меньшее время, чем потребовалось для создания.

## публичный ключ

Публичный ключ [пары ключей](terminology.md#keypair).

## корневой блок

Блок [](terminology.md#block) или [слот](terminology.md#slot), который достиг максимума [блокировки](terminology.md#lockout) на [валидаторе](terminology.md#validator). Корневой блок - это самый высокий блок, который является предком всех активных форков на валидаторе. Все исходные блоки корневого блока также являются переходным корневым блоком. Блоки, не являющиеся предком, а не потомком корневого блока, исключаются из рассмотрения на основе консенсуса и могут быть распущены.

## время работы

Компонент проверки [](terminology.md#validator) ответственный за выполнение [программы](terminology.md#program).

## фрагмент

Часть [ блока](terminology.md#block); наименьшее количество переданных между [валидаторами](terminology.md#validator).

## подпись

64-байтовая ed25519 подпись R (32-байт) и S (32-байт). С требованием о том, что R является упакованной точкой Edwards, не маленьким порядком и S является скалярным диапазоном 0 <= S < L. Это требование гарантирует, что подпись не будет признана. Каждая транзакция должна иметь по крайней мере одну подпись для [счета комиссии](terminology#fee-account). Таким образом, первая подпись в транзакции может рассматриваться как [трансактоновый id](terminology.md#transaction-id)

## skipped slot

A past [slot](terminology.md#slot) that did not produce a [block](terminology.md#block), because the leader was offline or the [fork](terminology.md#fork) containing the slot was abandoned for a better alternative by cluster consensus. A skipped slot will not appear as an ancestor for blocks at subsequent slots, nor increment the [block height](terminology#block-height), nor expire the oldest `recent_blockhash`.

Whether a slot has been skipped can only be determined when it becomes older than the latest [rooted](terminology.md#root) (thus not-skipped) slot.

## slot

The period of time for which each [leader](terminology.md#leader) ingests transactions and produces a [block](terminology.md#block).

Collectively, slots create a logical clock. Slots are ordered sequentially and non-overlapping, comprising roughly equal real-world time as per [PoH](terminology.md#proof-of-history).

## smart contract

A set of constraints that once satisfied, signal to a program that some predefined account updates are permitted.

## sol

The [native token](terminology.md#native-token) tracked by a [cluster](terminology.md#cluster) recognized by the company Solana.

## stake

Tokens forfeit to the [cluster](terminology.md#cluster) if malicious [validator](terminology.md#validator) behavior can be proven.

## supermajority

2/3 of a [cluster](terminology.md#cluster).

## sysvar

A synthetic [account](terminology.md#account) provided by the runtime to allow programs to access network state such as current tick height, rewards [points](terminology.md#point) values, etc.

## thin client

A type of [client](terminology.md#client) that trusts it is communicating with a valid [cluster](terminology.md#cluster).

## tick

A ledger [entry](terminology.md#entry) that estimates wallclock duration.

## tick height

The Nth [tick](terminology.md#tick) in the [ledger](terminology.md#ledger).

## token

A scarce, fungible member of a set of tokens.

## tps

[Transactions](terminology.md#transaction) per second.

## transaction

One or more [instructions](terminology.md#instruction) signed by the [client](terminology.md#client) using one or more [keypairs](terminology.md#keypair) and executed atomically with only two possible outcomes: success or failure.

## transaction id

The first [signature](terminology.md#signature) in a [transaction](terminology.md#transaction), which can be used to uniquely identify the transaction across the complete [ledger](terminology.md#ledger).

## transaction confirmations

The number of [confirmed blocks](terminology.md#confirmed-block) since the transaction was accepted onto the [ledger](terminology.md#ledger). A transaction is finalized when its block becomes a [root](terminology.md#root).

## transactions entry

A set of [transactions](terminology.md#transaction) that may be executed in parallel.

## validator

A full participant in the [cluster](terminology.md#cluster) responsible for validating the [ledger](terminology.md#ledger) and producing new [blocks](terminology.md#block).

## VDF

See [verifiable delay function](terminology.md#verifiable-delay-function).

## verifiable delay function

A function that takes a fixed amount of time to execute that produces a proof that it ran, which can then be verified in less time than it took to produce.

## vote

See [ledger vote](terminology.md#ledger-vote).

## vote credit

A reward tally for [validators](terminology.md#validator). A vote credit is awarded to a validator in its vote account when the validator reaches a [root](terminology.md#root).

## wallet

A collection of [keypairs](terminology.md#keypair).

## warmup period

Some number of [epochs](terminology.md#epoch) after [stake](terminology.md#stake) has been delegated while it progressively becomes effective. During this period, the stake is considered to be "activating". More info about: [warmup and cooldown](cluster/stake-delegation-and-rewards.md#stake-warmup-cooldown-withdrawal)
