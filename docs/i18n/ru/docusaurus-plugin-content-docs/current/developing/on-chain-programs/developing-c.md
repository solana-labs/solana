---
title: "Разработка на C"
---

Solana поддерживает написание on-chain программ с помощью языков программирования программирования C и C++.

## Макет проекта

Проекты на C имеют следующую структуру:

```
/src/<program name>
/makefile
```

Файл `makefile` должен содержать:

```bash
OUT_DIR := <path to place to resulting shared object>
include ~/.local/share/solana/install/active_release/bin/sdk/bpf/c/bpf.mk
```

Bpf-sdk будет находиться в указанном выше месте если вы настроите окружение в соответствии с гайдом [Как собрать проект](#how-to-build).

Взгляните на пример [helloworld](https://github.com/solana-labs/example-helloworld/tree/master/src/program-c) программы на C.

## Как собрать проект

Сначала настройте окружение:

- Установите последнюю версию Rust stable с https://rustup.rs
- Установите новейшие инструменты командной строки Solana из https://docs.solana.com/cli/install-solana-cli-tools

Затем соберите программу с помощью make:

```bash
make -C <program directory>
```

## Как тестировать

Solana использует тестовый фреймворк [Criterion](https://github.com/Snaipe/Criterion) и тесты выполняются каждый раз, когда программа собирается [Как собрать проект](#how-to-build)].

Чтобы добавить тесты, создайте новый файл рядом с исходным файлом `test_<program name>.c` и заполняйте его тестовыми запросами criterion. Для примера см. [helloworld C tests](https://github.com/solana-labs/example-helloworld/blob/master/src/program-c/src/helloworld/test_helloworld.c) или [документацию Criterion](https://criterion.readthedocs.io/en/master) для информации о том, как писать тесты.

## Точка входа программы

Программы экспортируют известный символ точки входа, видимый рантайму Solana, и вызывает его при вызове программы. Solana поддерживает несколько [версий загрузчика BPF](overview.md#versions) и точки входа программ могут быть различными для разных загрузчиков. Программы должны быть написаны и предназначены для развертывания в один тот же загрузчик. Для получения дополнительной информации смотрите [обзор загрузчиков](overview#loaders).

На данный момент существуют два поддерживаемых загрузчика [BPF Загрузчик](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/bpf_loader.rs#L17) и [Устаревший BPF загрузчик](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/bpf_loader_deprecated.rs#L14)

Оба они имеют одно и то же определение точки входа, то есть необработанный символ, который видит и вызывает рантам:

```c
extern uint64_t entrypoint(const uint8_t *input)
```

Эта точка берет общий байтовый массив, который содержит сериализованные параметры программы (идентификатор программы, аккаунты, данные инструкции и т. д.). Для десериализации параметров каждый загрузчик содержит собственную [вспомогательную функцию](#Serialization).

Обратитесь к [использованию точки входа из примера helloworld ](https://github.com/solana-labs/example-helloworld/blob/bc0b25c0ccebeff44df9760ddb97011558b7d234/src/program-c/src/helloworld/helloworld.c#L37) в качестве примера того, как это все может работать вместе.

### Сериализация

Обратитесь к [использованию функции десериализации в примере helloworld ](https://github.com/solana-labs/example-helloworld/blob/bc0b25c0ccebeff44df9760ddb97011558b7d234/src/program-c/src/helloworld/helloworld.c#L43).

Каждый загрузчик предоставляет вспомогательную функцию, которая десериализирует входные параметры программы в типы С:

- [Десериализация в загрузчике BPF](https://github.com/solana-labs/solana/blob/d2ee9db2143859fa5dc26b15ee6da9c25cc0429c/sdk/bpf/c/inc/solana_sdk.h#L304)
- [Десериализация в устаревшем BPF загрузчике](https://github.com/solana-labs/solana/blob/8415c22b593f164020adc7afe782e8041d756ddf/sdk/bpf/c/inc/deserialize_deprecated.h#L25)

Некоторые программы могут иметь собственную модель десериализации, для чего необходимо имплементировать собственную точку входа [raw entrypoint](#program-entrypoint). Обратите внимание, что функции десериализации сохраняют ссылки на сериализованный массив байтов для переменных, которые программа может изменять (балансы, данные аккаунта). Причиной этого является то, что после возврата загрузчик прочитает эти изменения, чтобы они могли быть приняты. Если программа реализует свою собственную функцию десериализации, то она должна удостовериться в том, что любые изменения программы могут быть записаны обратно в входной массив байтов.

Подробности о том, как сериализует ввод программы в документацию [Сериализация параметров ввода](overview.md#input-parameter-serialization).

## Типы данных

Вспомогательная функция десериализации загрузчика заполняет структуру [SolParameters](https://github.com/solana-labs/solana/blob/8415c22b593f164020adc7afe782e8041d756ddf/sdk/bpf/c/inc/solana_sdk.h#L276):

```c
/**
 * Structure that the program's entrypoint input data is deserialized into.
 */
typedef struct {
  SolAccountInfo* ka; /** Pointer to an array of SolAccountInfo, must already
                          point to an array of SolAccountInfos */
  uint64_t ka_num; /** Number of SolAccountInfo entries in `ka` */
  const uint8_t *data; /** pointer to the instruction data */
  uint64_t data_len; /** Length in bytes of the instruction data */
  const SolPubkey *program_id; /** program_id of the currently executing program */
} SolParameters;
```

'ka' - упорядоченный массив аккаунтов на которые ссылается инструкция и представлены как структуры [SolAccountInfo](https://github.com/solana-labs/solana/blob/8415c22b593f164020adc7afe782e8041d756ddf/sdk/bpf/c/inc/solana_sdk.h#L173). Местоположение аккаунта в массиве отражает его значение, например, при передаче баланса инструкция может определить первый аккаунт как источник, а второй - как место назначения.

Элементы структуры `SolAccountInfo` доступны только для чтения, за исключением баланса и данных `lamports` и `data`. Оба могут быть изменены программой в соответствии с [политикой выполнения рантайма](developing/programming-model/accounts. md#policy). Когда инструкция ссылается на аккаунт несколько раз, могут появиться дубликаты структуры `SolAccountInfo` но они все ссылаются на исходный входной массив байт. Программа должна работать с такими ситуациями деликатно, чтобы избежать перекрытия чтения/записи в один и тот же буфер. Если программа реализует свою функцию десериализации, то она также должна соответствующим образом обрабатывать дубликаты аккаунтов.

`data` представляют собой массив байтов [собственных данных инструкции](developing/programming-model/transactions.md#instruction-data).

`program_id` является открытым ключом исполняемой в настоящее время программы.

## Куча

C программы могут выделять память через системный вызов [`calloc`](https://github.com/solana-labs/solana/blob/c3d2d2134c93001566e1e56f691582f379b5ae55/sdk/bpf/c/inc/solana_sdk.h#L245) или реализовывать собственную кучу поверх 32KB области, начиная с виртуального адреса x30000000000. Регион кучи также используется `calloc`, так что если программа реализует собственную кучу, то она не должна вызвать `calloc`.

## Логирование

Рантайм предоставляет два системных вызова, которые принимают данные и логи, которые пишут в лог программы.

- [`sol_log(const char*)`](https://github.com/solana-labs/solana/blob/d2ee9db2143859fa5dc26b15ee6da9c25cc0429c/sdk/bpf/c/inc/solana_sdk.h#L128)
- [`sol_log_64(uint64_t, uint64_t, uint64_t, uint64_t, uint64_t)`](https://github.com/solana-labs/solana/blob/d2ee9db2143859fa5dc26b15ee6da9c25cc0429c/sdk/bpf/c/inc/solana_sdk.h#L134)

В разделе [отладка](debugging.md#logging) содержится дополнительная информация о работе с журналами программ.

## Бюджет Вычислений

Используйте системный вызов [`sol_log_compute_units()`](https://github.com/solana-labs/solana/blob/d3a3a7548c857f26ec2cb10e270da72d373020ec/sdk/bpf/c/inc/solana_sdk.h#L140) для получения сообщения, содержащего оставшееся количество вычислительных единиц, которое программа может потратить до остановки выполнения

See [compute budget](developing/programming-model/runtime.md#compute-budget) for more information.

## Дамп ELF

Содержимое общего объекта BPF может быть сохранено в формате текстового файла чтобы углубиться больше в композицию программы и ее поведение в рантайме. Дамп будет содержать информацию ELF, а также список всех символов и инструкций, которые их реализуют. Некоторые из сообщений лога ошибок BPF будут ссылаться на конкретные номера инструкций, где произошла ошибка. Эти ссылки можно посмотреть в дампе ELF, чтобы определить нарушающие инструкции и их контекст.

Для создания файла дампа:

```bash
$ cd <program directory>
$ make dump_<program name>
```

## Примеры

Репозитарий github [Solana Program Library](https://github.com/solana-labs/solana-program-library/tree/master/examples/c) содержит коллекцию примеров программ на C
