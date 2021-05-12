---
title: Данные Sysvar Cluster
---

Solana предоставляет различные данные о состоянии кластера программам через [`sysvar`](terminology.md#sysvar) аккаунты. Эти учетные записи заполняются на известных адресов, опубликованных вместе с макетами учетной записи [`solana-program` crate](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/sysvar/index.html), и излагается ниже.

Чтобы включить данные sysvar в программные операции, отправьте адрес учетной записи sysvar в список аккаунтов в транзакции. Учетная запись может быть прочитана в процессоре инструкций, как и в любой другой учетной записи. Access to sysvars accounts is always _readonly_.

## Часы

В sysvar Clock содержатся данные о времени кластера, включая текущий слот, epoch, и оценочные значения таймера Unix часов. Он обновляется каждый слот.

- Адрес: `SysvarC1ock1111111111111111111111111111111111`
- Макет: [Часы](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/clock/struct.Clock.html)
- Поля:

  - `слот`: текущий слот
  - `epoch_start_timestamp`: метка времени Unix первого слота в этой эпохе. В первом слоте эпохи, эта метка времени идентична `unix_timestamp` (ниже).
  - `эпоха`: текущая эпоха
  - `leader_schedule_epoch`: последняя эпоха, для которой лидер уже создан
  - `unix_timestamp`: метка времени Unix этого слота.

  Каждая ячейка имеет оценочную длительность на основе Proof of History. Но в действительности, слотов могут пройти быстрее и медленнее, чем эта оценка. В результате Unix сгенерирована временная метка слота на основе ввода оракула от валидаторов голосов. Эта отметка времени рассчитывается как медиана, взвешенная по цене цены и времени оценки, представленные голосами, ограниченный ожидаемым временем с момента начала эпохи.

  Более явно: для каждого слота, самая последняя отметка времени голосования, предоставленная каждым валидатором используется для генерации временной отметки за текущий слот (устаревшие слоты, поскольку временная метка считается Банк::ns_per_slot). Каждая оценочная отметка времени связана с долей делегирована этому счету голосования, чтобы создать распределение временных меток по долю. Медиана временная метка используется в качестве `unix_timestamp`, если не прошло времени с тех пор, как `epoch_start_timestamp` отклонился от ожидаемого просроченного времени более чем на 25%.

## EpochSchedule

The EpochSchedule sysvar contains epoch scheduling constants that are set in genesis, and enables calculating the number of slots in a given epoch, the epoch for a given slot, etc. (Note: the epoch schedule is distinct from the [`leader schedule`](terminology.md#leader-schedule))

- Адрес: `SysvarEpochSchedu1e1111111111111111111111`
- Макет: [EpochSchedule](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/epoch_schedule/struct.EpochSchedule.html)

## Комиссия

Системная переменная Fees содержит калькулятор комиссий для текущего слота. Он обновляется каждый слот, основываясь на правильном уровне комиссий.

- Адрес: `SysvarFees1111111111111111111111111111111`
- Макет: [Часы](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/sysvar/fees/struct.Fees.html)

## Инструкции

Инструкции sysvar содержат серийные инструкции в сообщении, пока, что сообщение обрабатывается. Это позволяет руководству программы ссылаться на другие инструкции в той же транзакции. Подробнее об Интроспективности [инструкции](implemented-proposals/instruction_introspection.md).

- Адрес: `Sysvar1nstructions11111111111111111111111`
- Макет: [Часы](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/sysvar/instructions/struct.Instructions.html)

## Недавние хэши

Системная переменная RecentBlockhashes содержит активные недавние хэши блоков, а также связанные с ними калькуляторы комиссий. Он обновляется каждый слот.

- Адрес: `SysvarRecentB1ockHashes111111111111111111`
- Макет: [Часы](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/sysvar/recent_blockhashes/struct.RecentBlockhashes.html)

## Аренда

В Rent sysvar есть арендная ставка. В настоящее время частота статична и установлена в генезе. Процент сгорания аренды изменяется с помощью ручной активации.

- Адрес: `SysvarRent11111111111111111111111111111`
- Макет: [Часы](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/rent/struct.Rent.html)

## SlotHashes

Sysvar SlotHashes содержит последние хэши родительского слота банков. Он обновляется каждый слот.

- Адрес: `SysvarS1otHashes11111111111111111111111`
- Макет: [Часы](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/slot_hashes/struct.SlotHashes.html)

## SlotHistory

Sysvar SlotHistory содержит битвектор слотов, присутствующих за последнюю эпоху. Он обновляется каждый слот.

- Адрес: `SysvarS1otHistory111111111111111111111111`
- Макет: [Часы](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/slot_history/struct.SlotHistory.html)

## StakeHistory

В системе StakeHistory содержится история активаций по всему кластеру и деактиваций по каждому эпоху. Он обновляется в начале каждой эпохи.

- Адрес: `SysvarStakeHistory1111111111111111111`
- Макет: [Часы](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/stake_history/struct.StakeHistory.html)
