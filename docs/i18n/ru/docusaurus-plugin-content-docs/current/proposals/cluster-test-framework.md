---
title: Тестовый фреймворк для кластера
---

В этом документе предлагается структура кластерного тестирования \ (CTF \). CTF это тестовая среда, которая позволяет тестировать выполнение с локальным, процессным кластером или развернутым кластером.

## Мотивация

Цель CTF - предоставить основу для написания тестов независимо от того, где и как развернут кластер. В этих тестах могут быть зафиксированы регрессии, и тесты могут быть запущены на развернутых кластерах для проверки развертывания. В центре внимания этих тестов должны быть стабильность кластера, консенсус, отказоустойчивость, стабильность API.

Тесты должны проверять единственную ошибку или сценарий и должны быть написаны с наименьшим количеством внутренних трубопроводов, подверженных тестированию.

## Обзор дизайна

Тесты предоставляют точку входа, которая представляет собой структуру `contact_info:: ContactInfo`, и пару ключей, которая уже была профинансирована.

Каждый узел в кластере настраивается с валидатором `:ValidatorConfig` во время загрузки. Во время загрузки эта конфигурация определяет любую дополнительную конфигурацию кластера, необходимую для тестирования. Кластер должен загружаться с конфигурацией, когда он запускается в процессе или в центре обработки данных.

После загрузки тест найдет кластер через точку входа gossip и настроет любые поведения, выполняемые через RPC валидатор.

## Тестирование интерфейса

Каждый тест CTF начинается с непрозрачной точки входа и финансируемой пары ключей. Тест не должен зависеть от того, как установлен кластер, и должен быть способен осуществлять всю функциональность кластера через общедоступные интерфейсы.

```text
use crate::contact_info::ContactInfo;
use solana_sdk::signature::{Keypair, Signer};
pub fn test_this_behavior(
    entry_point_info: &ContactInfo,
    funding_keypair: &Keypair,
    num_nodes: usize,
)
```

## Открытие кластера

На момент запуска теста кластер уже был установлен и полностью подключен. Тест может обнаружить большинство доступных узлов за несколько секунд.

```text
use crate::gossip_service::discover_nodes;

// Discover the cluster over a few seconds.
let cluster_nodes = discover_nodes(&entry_point_info, num_nodes);
```

## Конфигурация кластера

Для включения конкретных сценариев кластер должен загружаться со специальными конфигурациями. Эти конфигурации могут быть зафиксированы в `validator::ValidatorConfig`.

Пример:

```text
let mut validator_config = ValidatorConfig::default();
validator_config.rpc_config.enable_validator_exit = true;
let local = LocalCluster::new_with_config(
                num_nodes,
                10_000,
                100,
                &validator_config
                );
```

## Как создать новый тест

Например, есть ошибка, которая показывает, что кластер выходит из строя, когда он заполнен недействительными объявленными узлами gossip. Наша библиотека и протокол Gossip может измениться, но кластер все еще должен оставаться устойчивым к заполнениям недействительных объявленных узлов Gossip.

Настройка RPC сервиса:

```text
let mut validator_config = ValidatorConfig::default();
validator_config.rpc_config.enable_rpc_gossip_push = true;
validator_config.rpc_config.enable_rpc_gossip_refresh_active_set = true;
```

Подключите RPC и напишите новый тест:

```text
pub fn test_large_invalid_gossip_nodes(
    entry_point_info: &ContactInfo,
    funding_keypair: &Keypair,
    num_nodes: usize,
) {
    let cluster = discover_nodes(&entry_point_info, num_nodes);

    // Poison the cluster.
    let client = create_client(entry_point_info.client_facing_addr(), VALIDATOR_PORT_RANGE);
    for _ in 0..(num_nodes * 100) {
        client.gossip_push(
            cluster_info::invalid_contact_info()
        );
    }
    sleep(Durration::from_millis(1000));

    // Force refresh of the active set.
    for node in &cluster {
        let client = create_client(node.client_facing_addr(), VALIDATOR_PORT_RANGE);
        client.gossip_refresh_active_set();
    }

    // Verify that spends still work.
    verify_spends(&cluster);
}
```
