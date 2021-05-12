---
title: Framework de Teste de Cluster
---

Este documento propõe o Framework de Teste do Cluster \(CTF\). CTF é uma clava de testes que permite a execução de testes em um cluster local, em processo ou em um cluster implantado.

## Motivação

O objectivo do CTF é proporcionar um quadro para a elaboração de testes independentes do local e da forma como o cluster é implantado. Regressões podem ser capturadas nestes testes e os testes podem ser executados contra clusters implantados para verificar a implantação. O foco destes testes deve ser na estabilidade dos agrupamentos, no consenso, na tolerância de falha, na estabilidade da API.

Os testes devem verificar um único bug ou cenário e devem ser escritos com a quantidade mínima de canalização interna exposta ao teste.

## Visão Geral

É fornecido um ponto de entrada, que é uma estrutura `contact_info::ContactInfo` e um par de chaves que já foi financiado.

Cada nó no cluster é configurado com um `validator::ValidatorConfig` na hora da inicialização. Na inicialização esta configuração especifica qualquer configuração extra de cluster necessária para o teste. O cluster deve inicializar com a configuração quando estiver sendo executado em processo ou em um data center.

Uma vez iniciado, o teste descobrirá o cluster através de um ponto de entrada de gossip e configurará qualquer comportamento em tempo de execução através do validador RPC.

## Testar a interface

Cada teste CTF começa com um ponto de entrada opaco e um par de chaves financiado. O teste não deve depender de como o cluster é implantado, e deve ser capaz de exercer todas as funcionalidades de cluster através das interfaces disponíveis publicamente.

```text
use crate::contact_info::ContactInfo;
use solana_sdk::signature::{Keypair, Signer};
pub fn test_this_behavior(
    entry_point_info: &ContactInfo,
    funding_keypair: &Keypair,
    num_nodes: usize,
)
```

## Descoberta de cluster

No início dos testes, o cluster já foi estabelecido e está totalmente ligado. O teste pode descobrir a maioria dos nós disponíveis ao longo de alguns segundos.

```text
use crate::gossip_service::discover_nodes;

// Descubra o cluster ao longo de alguns segundos.
let cluster_nodes = descobrir_nodes(&entry_point_info, num_nós);
```

## Configuração do cluster

Para habilitar cenários específicos, o cluster precisa ser inicializado com configurações especiais. Estas configurações podem ser capturadas em `validator::ValidatorConfig`.

Por exemplo:

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

## Como projetar um novo teste

Por exemplo, há um bug que mostra que o cluster falha quando é inundado com nós de fofocas publicitados inválidos. Nossa biblioteca e nosso protocolo de fofocas podem mudar, mas o cluster ainda precisa permanecer resiliente a inundações de nós de fofocas publicitados inválidos.

Configurar o serviço RPC:

```text
let mut validator_config = ValidatorConfig::default();
validator_config.rpc_config.enable_rpc_gossip_push = true;
validator_config.rpc_config.enable_rpc_gossip_refresh_active_set = true;
```

Conecte os RPCs e escreva um novo teste:

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

    // Verificar que os gastos ainda funcionam.
    verificar_gastos(&cluster);
}
```
