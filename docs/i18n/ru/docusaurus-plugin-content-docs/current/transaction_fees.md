---
title: Transaction Fees
---

**С учетом изменений.**

Each transaction sent through the network, to be processed by the current leader validation-client and confirmed as a global state transaction, contains a transaction fee. Плата за транзакции дает много преимуществ в экономическом плане Solana, например:

- предоставить единичную компенсацию сети валидатора за ресурсы CPU / GPU, необходимые для обработки транзакции состояния,
- снижение сетевого спама, введя реальную стоимость транзакций,
- и обеспечить потенциальную долгосрочную экономическую стабильность сети через фиксированную протоколом минимальную комиссию за транзакцию, как описано ниже.

Network consensus votes are sent as normal system transfers, which means that validators pay transaction fees to participate in consensus.

Многие современные блокчейн-экономики (например, Bitcoin, Ethereum) полагаются на вознаграждения на основе протоколов для поддержки экономики в краткосрочной перспективе, с предположением, что доход, полученный за счет комиссий за транзакции, будет поддерживать экономику в долгосрочной перспективе, когда протокол полученные вознаграждения истекают. In an attempt to create a sustainable economy through protocol-based rewards and transaction fees, a fixed portion (initially 50%) of each transaction fee is destroyed, with the remaining fee going to the current leader processing the transaction. Запланированный глобальный уровень инфляции обеспечивает источник вознаграждений, распределяемых среди клиентов, проводящих валидацию, посредством описанного выше процесса.

Плата за транзакцию устанавливается сетевым кластером на основе прошлой пропускной способности, см. [ Комиссия за перегрузку ](implemented-proposals/transaction-fees.md#congestion-driven-fees). This minimum portion of each transaction fee can be dynamically adjusted depending on historical _signatures-per-slot_. Таким образом, протокол может использовать минимальную комиссию для целевого использования аппаратного обеспечения. By monitoring a protocol specified _signatures-per-slot_ with respect to a desired, target usage amount, the minimum fee can be raised/lowered which should, in turn, lower/raise the actual _signature-per-slot_ per block until it reaches the target amount. Этот процесс корректировки может считаться похожим на алгоритм корректировки сложности в протоколе Bitcoin, однако в этом случае минимальная комиссия за транзакцию корректирует использование аппаратного обеспечения обработки транзакций на желаемый уровень.

Как уже отмечалось, фиксированная доля каждой комиссии за транзакцию должна быть уничтожена. The intent of this design is to retain leader incentive to include as many transactions as possible within the leader-slot time, while providing an inflation limiting mechanism that protects against "tax evasion" attacks \(i.e. side-channel fee payments\).

Кроме того, при выборе форка можно учитывать сожженные сборы. В случае форка PoH со злонамеренным лидером цензуры мы ожидаем, что общая сумма уничтоженных сборов будет меньше, чем сопоставимый форк, из-за потерь сборов от цензуры. Если лидер цензуры должен компенсировать эти потерянные протокольные сборы, ему придется самостоятельно заменить сожженные сборы на своей вилке, что потенциально снижает стимул к цензуре в первую очередь.
