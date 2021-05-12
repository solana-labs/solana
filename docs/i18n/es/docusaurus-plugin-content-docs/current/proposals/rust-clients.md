---
title: Clientes Rust
---

## Problema

Las pruebas de alto nivel, como las de bench-tps, están escritas en términos del rasgo `Cliente`. Cuando ejecutamos estas pruebas como parte de la suite de pruebas, utilizamos la implementación de bajo nivel `BankClient`. Cuando necesitamos ejecutar la misma prueba contra un clúster, usamos la implementación de `ThinClient`. El problema con ese enfoque es que significa que el rasgo se expandirá continuamente para incluir nuevas funciones de utilidad y todas sus implementaciones necesitan agregar la nueva funcionalidad. Al separar el objeto de cara al usuario del rasgo que abstrae la interfaz de red, podemos ampliar el objeto de cara al usuario para incluir todo tipo de funcionalidades útiles, como el "spinner" de RpcClient, sin preocuparnos de tener que ampliar el trait y sus implementaciones.

## Solución propuesta

En lugar de implementar el rasgo de `Cliente`, `ThinClient` debe construirse con una implementación de él. De esa manera, todas las funciones de utilidad actualmente en el rasgo `Cliente` pueden moverse a `ThinClient`. `ThinClient` podría moverse a `solana-sdk` ya que todas sus dependencias de red estarían en la implementación de `Cliente`. Entonces añadiríamos una nueva implementación de `Client`, llamada `ClusterClient`, y eso viviría en la caja `solana-cliente` donde actualmente reside `ThinClient`.

Después de esta reorganización, cualquier código que necesite un cliente sería escrito en términos de `ThinClient`. En pruebas unitarias, la funcionalidad se invocaría con `ThinClient<BankClient>`, mientras que `main()` funciones, las pruebas de rendimiento y las invocarían con `ThinClient<ClusterClient>`.

Si los componentes de más alto nivel requieren más funcionalidad de lo que podría ser implementado por `BankClient`, debe ser implementado por un segundo objeto que implemente un segundo rasgo, siguiendo el mismo patrón descrito aquí.

### Manejo de errores

El `cliente` debe utilizar el enum `TransportError` existente para errores, excepto que el campo `Custom(String)` deba cambiarse a `Custom(Box<dyn Error>)`.

### Estrategia de implementación

1. Añade un nuevo objeto a `solana-sdk`, `RpcClientTng`, donde el sufijo `Tng` es temporal y significa "La próxima generación"
2. Inicializa `RpcClientTng` con una implementación `SyncClient`.
3. Añade un nuevo objeto a `solana-sdk`, `ThinClientTng`; inicialízalo con `RpcClientTng` y una implementación `AsyncClient`
4. Mover todas las unidades de pruebas desde `BankClient` a`ThinClientTng<BankClient>`
5. Añadir `ClusterClient`
6. Mover `ThinClient` usuarios a `ThinClientTng<ClusterClient>`
7. Eliminar `ThinClient` y renombrar `ThinClientTng` a `ThinClient`
8. Mover `RpcClient` usuarios a nuevos `ThinClient<ClusterClient>`
9. Eliminar `RpcClient` y renombrar `RpcClientTng` a `RpcClient`
