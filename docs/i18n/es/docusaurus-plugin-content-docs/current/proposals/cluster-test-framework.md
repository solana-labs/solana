---
title: Marco de pruebas de cluster
---

Este documento propone el Marco de Pruebas de Clúster \(CTF\). CTF es un arnés de prueba que permite ejecutar pruebas en un clúster local o un clúster desplegado.

## Motivación

El objetivo de CTF es proporcionar un marco para la escritura de pruebas independientes de dónde y cómo se despliegue el cluster. Las regresiones pueden ser capturadas en estas pruebas y las pruebas pueden ser ejecutadas contra los clusters desplegados para verificar el despliegue. Estas pruebas deben centrarse en la estabilidad del clúster, el consenso, la tolerancia a los fallos y la estabilidad de la API.

Las pruebas deben verificar un solo error o escenario, y deben ser escritas con la menor cantidad de tuberías internas expuestas a la prueba.

## Vista general del diseño

A las pruebas se les proporciona un punto de entrada, que es una estructura `contact_info::ContactInfo`, y un par de claves que ya ha sido financiado.

Cada nodo del clúster se configura con un `validator::ValidatorConfig` en el momento del arranque. En el momento del arranque, esta configuración especifica cualquier configuración adicional del clúster necesaria para la prueba. El clúster debe arrancar con la configuración cuando se ejecuta en proceso o en un centro de datos.

Una vez arrancada, la prueba descubrirá el clúster a través de un punto de entrada gossip y configurará cualquier comportamiento en tiempo de ejecución a través de RPC de validador.

## Probar interfaz

Cada prueba CTF comienza con un punto de entrada opaco y un par de claves financiado. La prueba no debe depender de cómo se despliega el clúster, y debe ser capaz de ejercitar toda la funcionalidad del clúster a través de las interfaces disponibles públicamente.

```text
use crate::contact_info::ContactInfo;
use solana_sdk::signature::{Keypair, Signer};
pub fn test_this_behavior(
    entry_point_info: &ContactInfo,
    funding_keypair: &Keypair,
    num_nodes: usizar,
)
```

## Descubrir cluster

Al iniciar la prueba, el clúster ya ha sido establecido y está completamente conectado. La prueba puede descubrir la mayoría de los nodos disponibles en un par de segundos.

```text
use crate::gossip_service::discover_nodes;

// Descubre el clúster durante unos segundos.
let cluster_nodes = discover_nodes(&entry_point_info, num_nodes);
```

## Configuración del Cluster

Para habilitar escenarios específicos, el clúster necesita ser arrancado con configuraciones especiales. Estas configuraciones pueden ser capturadas en `validator::ValidatorConfig`.

Por ejemplo:

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

## Cómo diseñar una nueva prueba

Por ejemplo, hay un error que muestra que el clúster falla cuando se llena de nodos publicitarios no válidos. Nuestra biblioteca y protocolo gossip pueden cambiar, pero el clúster todavía necesita ser resistente a las inundaciones de nodos publicitarios no válidos.

Configurar el servicio RPC:

```text
let mut validator_config = ValidatorConfig::default();
validator_config.rpc_config.enable_rpc_gossip_push = true;
validator_config.rpc_config.enable_rpc_gossip_refresh_active_set = true;
```

Envía los RPCs y escribe una nueva prueba:

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
    verify_spends(&clúster);
}
```
