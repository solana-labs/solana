---
title: Постоянное хранилище аккаунтов
---

## Постоянное хранилище аккаунтов

The set of accounts represent the current computed state of all the transactions that have been processed by a validator. Каждый валидатор должен поддерживать весь этот набор. Each block that is proposed by the network represents a change to this set, and since each block is a potential rollback point, the changes need to be reversible.

Постоянные хранилища, такие как NVME, в 20-40 раз дешевле, чем DDR. The problem with persistent storage is that write and read performance is much slower than DDR. Care must be taken in how data is read or written to. Both reads and writes can be split between multiple storage drives and accessed in parallel. This design proposes a data structure that allows for concurrent reads and concurrent writes of storage. Writes are optimized by using an AppendVec data structure, which allows a single writer to append while allowing access to many concurrent readers. The accounts index maintains a pointer to a spot where the account was appended to every fork, thus removing the need for explicit checkpointing of state.

## AppendVec

AppendVec - это структура данных, которая позволяет выполнять одновременное множественное чтение, с добавлением новых записей со стороны лишь одного процесса. Рост или изменение емкости AppendVec требует исключительного доступа. Это реализовано с помощью атомарного ` смещения`, которое обновляется в конце завершенной опперации записи.

Основанная для AppendVec память - это файл, который отображен в памяти. Файлы с отображением в память обеспечивают быстрый произвольный доступ, а подкачка страниц обрабатывается ОС.

## Индексирование аккаунтов

Индекс аккаунта предназначен для поддержки единого индекса для всех текущих учетных записей находящихся в других форках.

```text
type AppendVecId = usize;

type Fork = u64;

struct AccountMap(Hashmap<Fork, (AppendVecId, u64)>);

type AccountIndex = HashMap<Pubkey, AccountMap>;
```

Индекс аккаунта соответствует значению его публичного ключа, конкретного форка и местоположения данных аккаунта в AppendVec. Для получения версии аккаунта для определенного Форка:

```text
/// Load the account for the pubkey.
/// This function will load the account from the specified fork, falling back to the fork's parents
/// * fork - a virtual Accounts instance, keyed by Fork.  Accounts keep track of their parents with Forks,
///       the persistent store
/// * pubkey - The Account's public key.
pub fn load_slow(&self, id: Fork, pubkey: &Pubkey) -> Option<&Account>
```

Чтение удовлетворяется указанием отображенного в память местоположения в ` AppendVecId` с сохраненным смещением. Ссылка может быть возвращена без копирования.

### Корневые форки

[Tower BFT](tower-bft.md) в конечном итоге выбирает форк в качестве корневого форка и он сжимается. Нельзя откатиться обратно сжатый/корневой форк.

Когда форк сжимается, все аккаунты его предков, которые еще не присутствуют в форке, подтягиваются к форку путем обновления индексов. Аккаунты с нулевым балансом в жатом форке удаляются во время обновления индексов.

Аккаунт может быть подхвачен _сборщиком мусора_, если сжатие сделало его недоступным.

Существуют три возможных варианта:

- Поддержка HashSet корневых форков. Ожидается, что один будет создаваться каждую секунду. Все дерево таких форков в дальнейшем может быть удалено сборщиком-мусора. В качестве альтернативы, если каждый форк хранит счетчик ссылок на аккаунты, сборка мусора может происходить в любое время при обновлении местоположения индекса.
- Удаление из индекса всех обрезанных форков. Любые оставшиеся форки короче, чем кореневой, можно считать корневыми.
- Перенос любого старого корневого форка в новый после сканирования индексов. Любые оставшиеся форки короче нового корневого форка могут быть удалены позже.

## Garbage collection

As accounts get updated, they move to the end of the AppendVec. Once capacity has run out, a new AppendVec can be created and updates can be stored there. Eventually references to an older AppendVec will disappear because all the accounts have been updated, and the old AppendVec can be deleted.

To speed up this process, it's possible to move Accounts that have not been recently updated to the front of a new AppendVec. This form of garbage collection can be done without requiring exclusive locks to any of the data structures except for the index update.

The initial implementation for garbage collection is that once all the accounts in an AppendVec become stale versions, it gets reused. The accounts are not updated or moved around once appended.

## Index Recovery

Each bank thread has exclusive access to the accounts during append, since the accounts locks cannot be released until the data is committed. But there is no explicit order of writes between the separate AppendVec files. To create an ordering, the index maintains an atomic write version counter. Each append to the AppendVec records the index write version number for that append in the entry for the Account in the AppendVec.

To recover the index, all the AppendVec files can be read in any order, and the latest write version for every fork should be stored in the index.

## Snapshots

To snapshot, the underlying memory-mapped files in the AppendVec need to be flushed to disk. The index can be written out to disk as well.

## Performance

- Запись только с добавлением является достаточно быстрой. SSD и NVME, а также все структуры данных ядра уровня ОС позволяют приложениям работать с такой скоростью, которую позволяет пропускная способность PCI или NVMe \(2,700 МБ/с\).
- Каждый воспроизводящий и банковский поток записывают параллельно в свои собственные AppendVec файлы.
- Каждый AppendVec потенциально может быть размещен на отдельных NVMe.
- Каждый воспроизводящий и банковский поток имеют параллельный доступ ко всем AppendVecs файлам без блокировки возможности писать в них.
- Индекс требует блокировки записи для добавления новой информации. Однопоточная производительность обновлений HashMap составляет порядка 10 млн в секунду.
- Этапы Banking и Replay должны использовать 32 потока на каждый NVMe. NVM обладают оптимальной производительностью при одновременном чтении и записи 32 процессами.
