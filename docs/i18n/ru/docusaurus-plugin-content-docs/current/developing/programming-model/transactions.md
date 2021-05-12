---
title: "Транзакции"
---

Выполнение программы начинается с добавления [транзакции](terminology.md#transaction) в кластер. Рантайм Solana запускает программу для автоматической обработки всех [инструкций](terminology.md#instruction) содержащихся в транзакции, по порядку.

## Анатомия транзакции

Этот раздел описывает бинарный формат транзакции.

### Формат транзакции

Транзакция содержит массив [compact-array](#compact-array-format) подписей, и [сообщение](#message-format). Каждый элемент в массиве подписей — это [цифровая подпись](#signature-format) заданного сообщения. Рантайм Solana проверяет чтобы количество подписей соответствовало числу в первых 8 битах [заголовка сообщения](#message-header-format). Рантайм так же верифицирует, что каждая подпись была подписана приватным ключом, соответствующим публичному ключу в том же индексе массива адресов аккаунта сообщения.

#### Формат подписи

Каждая цифровая подпись имеет двоичный формат ed25519 и занимает 64 байта.

### Формат сообщения

Сообщение содержит [заголовок](#message-header-format), за которым следует массив [адресов аккаунта](#account-addresses-format), затем идет хеш блока [blockhash](#blockhash-format), и далее массив [инструкций](#instruction-format).

#### Формат заголовка сообщения

Заголовок сообщения содержит три неподписанных 8-битных значения. Первое значение - это число необходимых подписей в родительской транзакции. Второе значение — это количество адресов соответствующего аккаунта, доступное только для чтения. Третье значение в заголовке сообщения - это число доступных только для чтения адресов аккаунта не требующих подписей.

#### Формат адресов аккаунта

Адреса, которые требуют подписи, появляются в начале массива адресов аккаунта, затем идут адреса, запрашивающими доступ на запись и далее идут аккаунты доступные только для чтения. Адреса, которые не требуют подписей, следуют за адресами, которые требуют, также с аккаунтами открытыми для чтения и записи, за которыми следуют открытые только для чтения аккаунты.

#### Формат хеша блока

В элементе blockhash содержится 32 байт SHA-256 хэш блока. Он используется как индикация, когда клиент последний раз просматривал блокчейн. Валидаторы могут отклонять транзакции, если блокхэш устарел.

### Формат инструкции

Инструкция содержит индекс id программы, затем идет компакт-массив индексов адресов аккаунта и далее компакт-массив непрозрачных 8-битных данных. Индекс id программы используется для идентификации on-chain программы, которая может интерпретировать непрозрачные данные. Индекс id программы - это неподписанный 8-битный индекс адреса аккаунта в массиве адресов аккаунта принадлежащего сообщению. Индексами адреса аккаунта являются неподписанные 8-битные индексы в том же самом массиве.

### Формат компакт-массива

Компакт-массив сериализируется как длинна массива, после чего следует каждый элемент массива. Длина массива - это специальная многобайтная кодировка под названием compact-u16.

#### Формат Compact-u16

Compact-u16 — многобайтовая кодировка 16 бит. Первый байт содержит нижние 7 бит значения в его нижних 7 битах. Если значение над 0x7f, устанавливается высокий бит, а следующие 7 битов значения помещаются в нижние 7 бит второго байта. Если значение над 0x3fff, устанавливается высокий бит и оставшихся 2 бита значения помещаются в нижние 2 бита третьего байта.

### Формат адресов аккаунта

Адрес аккаунта - это 32 байта произвольных данных. Когда адрес требует цифровой подписи, рантайм интерпретирует его как публичный ключ ил пары ключей ed25519.

## Инструкции

Каждая [инструкция](terminology.md#instruction) указывает одну программу, a подмножество аккаунтов транзакций, которые должны быть переданы программе, и массива байт, который так же передается программе. Программа интерпретирует массив данных и оперирует аккаунтами, указанными ее инструкциями. Программа может возвращать успешный результат или код ошибки. В случае ошибки происходит немедленный откат всей транзакции.

Programs typically provide helper functions to construct instructions they support. Например, системная программа предоставляет следующую вспомогательную функцию Rust для создания инструкции [`SystemInstruction::CreateAccount`](https://github.com/solana-labs/solana/blob/6606590b8132e56dab9e60b3f7d20ba7412a736c/sdk/program/src/system_instruction.rs#L63):

```rust
pub fn create_account(
    from_pubkey: &Pubkey,
    to_pubkey: &Pubkey,
    lamports: u64,
    space: u64,
    owner: &Pubkey,
) -> Instruction {
    let account_metas = vec![
        AccountMeta::new(*from_pubkey, true),
        AccountMeta::new(*to_pubkey, true),
    ];
    Instruction::new_with_bincode(
        system_program::id(),
        &SystemInstruction::CreateAccount {
            lamports,
            space,
            owner: *owner,
        },
        account_metas,
    )
}
```

Которую можно найти здесь:

https://github.com/solana-labs/solana/blob/6606590b8132e56dab9e60b3f7d20ba7412a736c/sdk/program/src/system_instruction.rs#L220

### Id программы

Содержащееся в инструкции [id программы](terminology.md#program-id) какая программа будет обрабатывать данную инструкцию. Владелец аккаунта программы определяет какой загрузчик должен использоваться для загрузки и выполнения программы, а данные содержат информацию о том, как рантайм будет запускать программу.

In the case of [on-chain BPF programs](developing/on-chain-programs/overview.md), the owner is the BPF Loader and the account data holds the BPF bytecode. Program accounts are permanently marked as executable by the loader once they are successfully deployed. The runtime will reject transactions that specify programs that are not executable.

Unlike on-chain programs, [Native Programs](developing/runtime-facilities/programs) are handled differently in that they are built directly into the Solana runtime.

### Аккаунты

Аккаунты, упомянутые инструкцией, представляют on-chain состояние и служат как входами, так и выходом программы. Подробнее об аккаунтах можно найти в разделе [Аккаунты](accounts.md).

### Данные инструкции

Каждая инструкция содержит массив байтов общего назначения, который передается программе вместе с аккаунтами. Содержимое данных инструкции специфично конкретной программе и обычно используется для передачи тех операций, которые должна выполнять программа, и любую дополнительную информацию, которую могут понадобиться эти операции в дальнейшем и помимо той, что содержат аккаунты.

Программы вольны выбирать кодировку информации в байтовом массиве данных инструкции. Выбор метода кодирования данных должен учитывать перекрытие декодирования, поскольку этот шаг выполняется on-chain программой. Было отмечено, что некоторые общие кодировки (например, двоичный код Rust) очень неэффективны.

Ренализация токена [Solana Program Library's Token program](https://github.com/solana-labs/solana-program-library/tree/master/token) представляет один из примеров того как данные инструкции могут быть кодированы эффективно, однако отметьте что этот метод поддерживает только типы фиксированного размера. Токен использует трест [Pack](https://github.com/solana-labs/solana/blob/master/sdk/program/src/program_pack.rs) для кодирования/декодирования данных инструкций и для инструкций токена, и для состояния аккаунта токена.

### Multiple instructions in a single transaction

A transaction can contain instructions in any order. This means a malicious user could craft transactions that may pose instructions in an order that the program has not been protected against. Programs should be hardened to properly and safely handle any possible instruction sequence.

One not so obvious example is account deinitialization. Some programs may attempt to deinitialize an account by setting its lamports to zero, with the assumption that the runtime will delete the account. This assumption may be valid between transactions, but it is not between instructions or cross-program invocations. To harden against this, the program should also explicitly zero out the account's data.

An example of where this could be a problem is if a token program, upon transferring the token out of an account, sets the account's lamports to zero, assuming it will be deleted by the runtime. If the program does not zero out the account's data, a malicious user could trail this instruction with another that transfers the tokens a second time.

## Подписи

Каждая транзакция явно перечисляет все публичные ключи аккаунта, на которые ссылаются инструкции транзакции. Подмножество этих публичных ключей сопровождается подписью транзакции. Эти подписи показывают on-chain программам, что владелец аккаунта авторизовал транзакцию. Обычно программа использует авторизацию, чтобы разрешить дебетование аккаунта или изменение его данных. Дополнительная информация о том, как авторизация взаимодействует с программой может быть найдена в [Аккаунтах](accounts.md#signers)

## Последний Blockhash

Транзакция включает в себя недавний [ блокхэш](terminology.md#blockhash), чтобы предотвратить дублирование и дать транзакциям время жизни. Любая транзакция, полностью идентичная предыдущей, будет отклонена, а добавление актуального блокхэша позволяет, например, нескольким транзакциям повторить одно и то же действие. Транзакции также имеют время жизни, определяемое блокхэшем (blockhash), так любые транзакции, чей blockhash устарел будут отклонены.
