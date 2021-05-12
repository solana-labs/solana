---
title: "Native Programs"
---

Solana contains a small handful of native programs, which are required to run validator nodes. Unlike third-party programs, the native programs are part of the validator implementation and can be upgraded as part of cluster upgrades. Могут происходить обновления для добавления возможностей, исправления ошибок или улучшения производительности. Интерфейс изменяется на отдельные инструкции, если когда-либо происходит. Вместо этого при необходимости изменения добавляются новые инструкции, а предыдущие отмечены устаревшими. Приложения могут обновиться на собственной шкале времени, не беспокоясь о перерывах между обновлениями.

For each native program the program id and description each supported instruction is provided. A transaction can mix and match instructions from different programs, as well include instructions from on-chain programs.

## Системная программа

Create new accounts, allocate account data, assign accounts to owning programs, transfer lamports from System Program owned accounts and pay transacation fees.

- Программа id: `1111111111111111111111111111111111`
- Инструкции: [Системная инструкция](https://docs.rs/solana-sdk/VERSION_FOR_DOCS_RS/solana_sdk/system_instruction/enum.SystemInstruction.html)

## Программа конфигурации

Добавить конфигурационные данные в цепочку и список открытых ключей, которым разрешено ее модифицировать

- Программа id: `Config11111111111111111111111111111111111111111`
- Инструкции: [Системная инструкция](https://docs.rs/solana-config-program/VERSION_FOR_DOCS_RS/solana_config_program/config_instruction/index.html)

В отличие от других программ, конфигурационная программа не определяет никаких отдельных инструкций. У него есть только одна неявная инструкция, инструкция "магазин". Ее инструкция данные представляют собой набор ключей, которые открывают доступ к аккаунту, и данных для хранения в нем.

## Программа стейка

Create and manage accounts representing stake and rewards for delegations to validators.

- Программа: `Stake11111111111111111111111111111111111111111111`
- Инструкции: [Инструкция по разбиванию](https://docs.rs/solana-stake-program/VERSION_FOR_DOCS_RS/solana_stake_program/stake_instruction/enum.StakeInstruction.html)

## Программа голосования

Create and manage accounts that track validator voting state and rewards.

- Программа id: `Голосование 111111111111111111111111111111111111111111111`
- Инструкции: [Голосование](https://docs.rs/solana-vote-program/VERSION_FOR_DOCS_RS/solana_vote_program/vote_instruction/enum.VoteInstruction.html)

## Загрузчик BPF

Deploys, upgrades, and executes programs on the chain.

- Program id: `BPFLoaderUpgradeab1e11111111111111111111111`
- Инструкции: [Голосование](https://docs.rs/solana-sdk/VERSION_FOR_DOCS_RS/solana_sdk/loader_upgradeable_instruction/enum.UpgradeableLoaderInstruction.html)

The BPF Upgradeable Loader marks itself as "owner" of the executable and program-data accounts it creates to store your program. When a user invokes an instruction via a program id, the Solana runtime will load both your the program and its owner, the BPF Upgradeable Loader. The runtime then passes your program to the BPF Upgradeable Loader to process the instruction.

[More information about deployment](cli/deploy-a-program.md)

## Программа Secp256k1

Проверить операции восстановления публичных ключей secp256k1 (ecrecover).

- Программа id: `KeccakSecp256k11111111111111111111111111111`
- Инструкции: [new_secp256k1_instruction](https://github.com/solana-labs/solana/blob/1a658c7f31e1e0d2d39d9efbc0e929350e2c2bcb/sdk/src/secp256k1_instruction.rs#L31)

Программа secp256k1 обрабатывает инструкцию, которая принимает в качестве первого байта количество следующих сериализированных в данных инструкций:

```
struct Secp256k1SignatureOffsets {
    secp_signature_key_offset: u16, // смещение на [signature,recovery_id, therum_address] из 64+1+20 байт
    secp_signature_instruction_index: u8, // индекс инструкции для поиска данных
    secp_pubkey_offset: u16, // смещение к [подписью, ecovery_id] из 64+1 байт
    secp_signature_instruction_index: u8, // Инструкция для поиска данных
    secp_message_data_offset: u16, // смещение до начала данных сообщения
    secp_message_data_size: u16, // размер данных сообщения
    secp_message_instruction_index: u8, // индекс данных инструкции для получения данных сообщения
}
```

Псевдо-код операции:

```
process_instruction() {
  для i в 0.. ount {
      // Обращенные значения индексов:
      инструкции = &транзакции. essage().instructions
      signature = instructions[secp_signature_instruction_index]. ata[secp_signature_offset..secp_signature_offset + 64]
      recovery_id = instructions[secp_signature_instruction_index]. ata[secp_signature_offset + 64]
      ref_eth_pubkey = instructions[secp_pubkey_instruction_index]. ata[secp_pubkey_offset..secp_pubkey_offset + 32]
      message_hash = keccak256(инструкции[secp_message_instruction_index].data[secp_message_data_offset.. ecp_message_data_offset + secp_message_data_size])
      pubkey = ecrecover(signature, recovery_id, message_hash)
      eth_pubkey = keccak256(pubkey[1..])[12..]
      if eth_pubkey != ref_eth_pubkey {
          return Error
      }
  }
  return Success
}
```

Это позволяет пользователю указать любые инструкции в транзакции для подписей и данных сообщений. При указании специальных инструкций sysvar, можно также получить данные из самой транзакции.

Стоимость транзакции будет рассчитывать количество подписей для проверки умноженного на множитель стоимости подписи.

### Замечания по оптимизации

Операция должна будет произойти после (хотя бы частичной) десериализации, но все входные данные поступают из самих данных транзакции, это позволяет относительно легко выполнять параллельно с обработкой транзакций и PoH проверкой.
