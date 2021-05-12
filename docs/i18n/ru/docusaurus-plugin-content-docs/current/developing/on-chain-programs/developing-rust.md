---
title: "Разработка на Rust"
---

Solana поддерживает написание on-chain программ, на языке программирования [Rust](https://www.rust-lang.org/).

## Макет проекта

Программы Solana на Rust соответствуют типовой [ структуре проекта Rust](https://doc.rust-lang.org/cargo/guide/project-layout.html):

```
/inc/
/src/
/Cargo.toml
```

Однако также должны включать:

```
/Xargo.toml
```

Который должен содержать:

```
[target.bpfel-unknown-unknown.dependencies.std]
features = []
```

Solana Rust programs may depend directly on each other in order to gain access to instruction helpers when making [cross-program invocations](developing/programming-model/calling-between-programs.md#cross-program-invocations). При этом очень важно не подтягивать в зависимые программы символы точек входа из зависимостей, поскольку они могут конфликтовать с собственными символами программы. To avoid this, programs should define an `exclude_entrypoint` feature in `Cargo.toml` and use to exclude the entrypoint.

- [Определение свойства](https://github.com/solana-labs/solana-program-library/blob/a5babd6cbea0d3f29d8c57d2ecbbd2a2bd59c8a9/token/program/Cargo.toml#L12)
- [Исключение точки входа](https://github.com/solana-labs/solana-program-library/blob/a5babd6cbea0d3f29d8c57d2ecbbd2a2bd59c8a9/token/program/src/lib.rs#L12)

Затем если программы включают другую программу как зависимость, то это должно быть реализовано с помощью `exclude_entrypoint` функционал.

- [Включение без точки входа](https://github.com/solana-labs/solana-program-library/blob/a5babd6cbea0d3f29d8c57d2ecbbd2a2bd59c8a9/token-swap/program/Cargo.toml#L19)

## Зависимости проекта

Как минимум, программы для Solana на Rust должны включать пакет [solana-program](https://crates.io/crates/solana-program).

Программы Solana BPF имеют некоторые [ограничения](#restrictions), которые могут помешать включению определенных пакетов в качестве зависимостей или требовать специальной обработки.

Например:

- Пакеты, которые требуют создания архитектуры входящей в подмножество официально поддерживаемых инструментов. Для них нет обхода ограничений зависимостей, если только этот пакет не является форком и BPF добавлен к требованиям их архитектуры.
- Пакеты Rust могут зависеть от `rand`, который не поддерживаются в детерминированной программной среде Solana. Для включения пакета `rand` в зависимости программы обратитесь к материалу [Depending on Rand](#depending-on-rand).
- Пакеты Rust могут переполнить стек даже в том случае, если код, переполняющий стек, не включен в саму программу непосредственно. За дополнительной информацией обратитесь к [Stack](overview.md#stack).

## Как собрать проект

Сначала настройте окружение:

- Установите последнюю версию Rust stable с https://rustup.rs/
- Установите новейшие инструменты командной строки Solana из https://docs.solana.com/cli/install-solana-cli-tools

Доступна нормальная сборка программ с cargo, а ваша хост-машина может быть использована для модульного тестирования:

```bash
$ cargo build
```

Для правильной сборки специфических on-chain программ, например таких как SPL токены для Solana BPF, способных разворачиваться в кластере, используйте:

```bash
$ cd <the program directory>
$ cargo build-bpf
```

## Как тестировать

Программы Solana могут быть протестированы с помощью обычного механизма `cargo test` путем непосредственной проверки программных функций.

Чтобы облегчить тестирование в среде, которая более тесно соответствует среде реального кластера, разработчики могут использовать пакет [`program-test`](https://crates.io/crates/solana-program-test). Пакет `program-tes` crate запускает локальный экземпляр рантайма и позволяет тестам отправлять несколько транзакций при сохранении состояния в течение теста.

Для получения дополнительной информации пример [test in sysvar example](https://github.com/solana-labs/solana-program-library/blob/master/examples/rust/sysvar/tests/functional.rs) демонстрирует, как инструкция, содержащая аккаунт syavar, отправляется и обрабатывается программой.

## Точка входа программы

Программы экспортируют известный символ точки входа, который просматривает рантайм Solana, и вызывает при вызове программы. Solana поддерживает несколько [версий загрузчика BPF](overview.md#versions) и точки входа программ могут быть различными для разных загрузчиков. Программы должны быть написаны и предназначены для развертывания в один тот же загрузчик. Для получения дополнительной информации смотрите [обзор загрузчиков](overview#loaders).

На данный момент существуют два поддерживаемых загрузчика [BPF Загрузчик](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/bpf_loader.rs#L17) и [Устаревший BPF загрузчик](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/bpf_loader_deprecated.rs#L14)

Оба они имеют одно и то же определение точки входа, то есть необработанный символ, который видит и вызывает рантам:

```rust
#[no_mangle]
pub unsafe extern "C" fn entrypoint(input: *mut u8) -> u64;
```

Эта точка берет общий байтовый массив, который содержит сериализованные параметры программы (идентификатор программы, аккаунты, данные инструкции и т. д.). Чтобы десериализовать параметры каждый загрузчик содержит свой собственный макрос который экспортирует исходную точку входа, дезактивирует параметры, вызывает определенную пользователем инструкцию, обрабатывает функцию и возвращает результаты.

Макрос точки входа находится здесь:

- [Макрос BPF загрузчика](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/entrypoint.rs#L46)
- [Макрос устаревшего BPF загрузчика](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/entrypoint_deprecated.rs#L37)

Программа определила функцию обработки инструкций, для которой вызов макроса точки входа должен иметь следующую форму:

```rust
pub type ProcessInstruction =
    fn(program_id: &Pubkey, accounts: &[AccountInfo], instruction_data: &[u8]) -> ProgramResult;
```

Обратитесь к [использованию точки входа из примера helloworld ](https://github.com/solana-labs/example-helloworld/blob/c1a7247d87cd045f574ed49aec5d160aefc45cf2/src/program-rust/src/lib.rs#L15) в качестве примера того, как это все может работать вместе.

### Десериализация параметров

Каждый загрузчик предоставляет вспомогательную функцию, которая десериализирует входные параметры программы в типы Rust. Макрос точки входа автоматически вызывает вспомогательную функцию для десериализации:

- [Десериализация в загрузчике BPF](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/entrypoint.rs#L104)
- [Десериализация в устаревшем BPF загрузчике](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/entrypoint_deprecated.rs#L56)

Некоторые программы могут иметь собственную модель десериализации, для чего необходимо имплементировать собственную точку входа [raw entrypoint](#program-entrypoint). Обратите внимание, что функции десериализации сохраняют ссылки на сериализованный массив байтов для переменных, которые программа может изменять (балансы, данные аккаунта). Причиной этого является то, что после возврата загрузчик прочитает эти изменения, чтобы они могли быть приняты. Если программа реализует свою собственную функцию десериализации, то она должна удостовериться в том, что любые изменения программы могут быть записаны обратно в входной массив байтов.

Подробности о том, как сериализует ввод программы в документацию [Сериализация параметров ввода](overview.md#input-parameter-serialization).

### Типы данных

В точке входа загрузчика макросы вызывают программный процессор инструкции со следующими параметрами:

```rust
program_id: &Pubkey,
accounts: &[AccountInfo],
instruction_data: &[u8]
```

Идентификатор программы является публичным ключом исполняемой программы.

Аккаунты, в данном контексте, - это упорядоченый слайс аккаунтов на которые ссылается инструкция и представлены как структуры [AccountInfo](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/account_info.rs#L10). Местоположение аккаунта в массиве отражает его значение, например, при передаче баланса инструкция может определить первый аккаунт как источник, а второй - как место назначения.

Элементы структуры `AccountInfo` доступны только для чтения, за исключением `lamports` и `data`. Оба могут быть изменены программой в соответствии с [политикой выполнения рантайма](developing/programming-model/accounts. md#policy). Эти члены защищены конструкцией Rust `RefCell`, поэтому они должны быть заимствованы для чтения или записи к ним. Причиной этого является то, что они оба возвращают оригинальный входной массив байтов, но могут быть несколько записей в аккаунтах, которые указывают на один и тот же аккаунт. Использование `RefCell` гарантирует, что программа не выполняет случайно перекрывающиеся чтение/запись в те же самые базовые данные через несколько структур `AccountInfo`. Если программа реализует свою функцию десериализации, то она также должна соответствующим образом обрабатывать дубликаты аккаунтов.

Данные инструкции представляют собой массив байтов [собственных данных инструкции](developing/programming-model/transactions.md#instruction-data).

## Куча

Программы Rust реализуют кучу через пользовательский [`global_allocator`](https://github.com/solana-labs/solana/blob/8330123861a719cd7a79af0544617896e7f00ce3/sdk/program/src/entrypoint.rs#L50)

Программы могут реализовывать собственные `global_allocator` на основе своих специфических потребностей. Обратитесь к [кастомный пример кучи](#examples) для получения дополнительной информации.

## Ограничения Rust

On-chain программы Rust поддерживают основной функционал языка - libstd, libcore, и liballoc, а так же большинство сторонних пакетов.

Есть некоторые ограничения, поскольку эти программы работают в однопоточной среде с ограниченными ресурсами и должны быть детерминированными:

- Нет доступа к
  - `rand`
  - `std::fs`
  - `std::net`
  - `std::os`
  - `std::future`
  - `std::net`
  - `std::process`
  - `std::sync`
  - `std::task`
  - `std::thread`
  - `std::time`
- Ограниченный доступ к:
  - `std::hash`
  - `std::os`
- Использовать Bincode чрезвычайно дорого в плане вычислительной производительности в течении обоих циклов и глубины вызова - поэтому этого стоит избегать
- Форматирование строк следует избегать, так как оно также вычислительно дорого.
- Нет поддержки инструкций `println!`, `print!`, вместо этого следует использовать [вспомогательные логгеры Solana](#logging).
- Рантайм лимитирует на количество инструкций, которое программа может выполнить в процессе обработки одной инструкции. See [computation budget](developing/programming-model/runtime.md#compute-budget) for more information.

## Rand зависимость в Rust

Детерминированный запуск программ ограничен, поэтому работа со случайными числами недоступна. Иногда программа может зависеть от пакета, который зависит от `rand` даже если программа не использует функции случайного числа. Если программа зависит от `rand`, то компиляция не будет выполнена, так как у Solana нет поддержки `get-random`. Ошибка обычно будет выглядеть так:

```
error: target is not supported, for more information see: https://docs.rs/getrandom/#unsupported-targets
   --> /Users/jack/.cargo/registry/src/github.com-1ecc6299db9ec823/getrandom-0.1.14/src/lib.rs:257:9
    |
257 | /         compile_error!("\
258 | |             target is not supported, for more information see: \
259 | |             https://docs.rs/getrandom/#unsupported-targets\
260 | |         ");
    | |___________^
```

Чтобы обойти это ограничение, добавьте следующую зависимость в `Cargo.toml`:

```
getrandom = { version = "0.1.14", features = ["dummy"] }
```

## Логирование

Макрос Rust `println!` требует большого вычислительного ресурса и не поддерживается. Вместо этого предоставляется вспомогательный макрос [`msg!`](https://github.com/solana-labs/solana/blob/6705b5a98c076ac08f3991bb8a6f9fcb280bf51e/sdk/program/src/log.rs#L33).

`msg!` имеет две формы:

```rust
msg!("A string");
```

или

```rust
msg!(0_64, 1_64, 2_64, 3_64, 4_64);
```

Обе выводят результат в логи программы. При желании программа может эмулировать `println!` используя `format!`:

```rust
msg!("Some variable: {:?}", variable);
```

В разделе [отладка](debugging.md#logging) содержится дополнительная информация о работе с логом программы [Примеры Rust](#examples) содержит пример логирования.

## Макросы Rust panic и assert

`panic!`, `assert!`, а так же их вывод печатаются в [логи программы](debugging.md#logging) по умолчанию.

```
INFO  solana_runtime::message_processor] Finalized account CGLhHSuWsp1gT4B7MY2KACqp9RUwQRhcUFfVSuxpSajZ
INFO  solana_runtime::message_processor] Call BPF program CGLhHSuWsp1gT4B7MY2KACqp9RUwQRhcUFfVSuxpSajZ
INFO  solana_runtime::message_processor] Program log: Panicked at: 'assertion failed: `(left == right)`
      left: `1`,
     right: `2`', rust/panic/src/lib.rs:22:5
INFO  solana_runtime::message_processor] BPF program consumed 5453 of 200000 units
INFO  solana_runtime::message_processor] BPF program CGLhHSuWsp1gT4B7MY2KACqp9RUwQRhcUFfVSuxpSajZ failed: BPF program panicked
```

### Пользовательский Обработчик Паники

Программы могут переопределить обработчик паники по умолчанию, предоставив собственную реализацию.

Сначала определите функцию `custom-panic` в `Cargo.toml`

```toml
[features]
default = ["custom-panic"]
custom-panic = []
```

Затем предоставьте пользовательскую реализацию обработчика паники:

```rust
#[cfg(all(feature = "custom-panic", target_arch = "bpf"))]
#[no_mangle]
fn custom_panic(info: &core::panic::PanicInfo<'_>) {
    solana_program::msg!("program custom panic enabled");
    solana_program::msg!("{}", info);
}
```

В примере выше показана реализация по умолчанию, но разработчики могут заменить этот код на другой, более удовлетворяющий их потребности.

Одним из побочных эффектов поддержки полных сообщений паники по умолчанию является то, что программы затрачивают на реализацию больше чем на реализацию `libstd` в общем объекте программы. Типовые программы, как правило итак используют `libstd` и могут не почувствовать значительного увеличения размера общего объекта. Но на программы, которые явно пытаются быть очень маленькими, избегая реализации `libstd`, его включение может существенно повлиять (~25kb) на размер программы в памяти. Чтобы устранить это влияние, программы могут обеспечить свой собственный пользовательский обработчик с пустой реализацией.

```rust
#[cfg(all(feature = "custom-panic", target_arch = "bpf"))]
#[no_mangle]
fn custom_panic(info: &core::panic::PanicInfo<'_>) {
    // Do nothing to save space
}
```

## Бюджет Вычислений

Используйте системный вызов [`sol_log_compute_units()`](https://github.com/solana-labs/solana/blob/d3a3a7548c857f26ec2cb10e270da72d373020ec/sdk/program/src/log.rs#L102) для получения сообщения, содержащего оставшееся количество вычислительных единиц, которое программа может потратить до остановки выполнения

See [compute budget](developing/programming-model/runtime.md#compute-budget) for more information.

## Дамп ELF

Содержимое общего объекта BPF может быть сохранено в формате текстового файла чтобы углубиться больше в композицию программы и ее поведение в рантайме. Дамп будет содержать информацию ELF, а также список всех символов и инструкций, которые их реализуют. Некоторые из сообщений лога ошибок BPF будут ссылаться на конкретные номера инструкций, где произошла ошибка. Эти ссылки можно посмотреть в дампе ELF, чтобы определить нарушающие инструкции и их контекст.

Для создания файла дампа:

```bash
$ cd <program directory>
$ cargo build-bpf --dump
```

## Примеры

Репозитарий github [Solana Program Library ](https://github.com/solana-labs/solana-program-library/tree/master/examples/rust) содержит коллекцию примеров программ на Rust.
