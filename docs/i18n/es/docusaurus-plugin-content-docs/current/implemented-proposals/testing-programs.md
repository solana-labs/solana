---
title: Programas de pruebas
---

Las aplicaciones envían transacciones a un cluster Solana y a los validadores de consultas para confirmar que las transacciones fueron procesadas y para comprobar el resultado de cada transacción. Cuando el clúster no se comporta como se esperaba, podría ser por varias razones:

- El programa tiene errores
- El cargador BPF rechazó una instrucción de programa insegura
- La transacción era demasiado grande
- La transacción fue inválida
- El tiempo de ejecución intentó ejecutar la transacción cuando otra estaba accediendo

  la misma cuenta

- La red ha dejado de realizar la transacción
- El clúster deshace el ledger
- Un validador respondió maliciosamente a las consultas

## Los rasgos de AsyncClient y SyncClient

Para solucionar problemas, la aplicación debe redirigir un componente de menor nivel, donde son posibles menos errores. La reasignación se puede realizar con diferentes implementaciones de los rasgos AsyncClient y SyncClient.

Los componentes implementan los siguientes métodos primarios:

```text
trait AsyncClient {
    fn async_send_transaction(&self, transaction: Transaction) -> io::Result<Signature>;
}

trait SyncClient {
    fn get_signature_status(&self, signature: &Signature) -> Result<Option<transaction::Result<()>>>;
}
```

Los usuarios envían transacciones y esperan los resultados de forma asincrónica y sincronizada.

### ThinClient para Clusters

La implementación de más alto nivel, ThinClient, apunta a un clúster Solana, que puede ser una red de pruebas desplegada o un clúster local corriendo en una máquina de desarrollo.

### TpuClient para el TPU

El siguiente nivel es la implementación de la TPU, que todavía no se ha implementado. A nivel de TPU, la aplicación envía transacciones a través de canales Rust, donde no puede haber sorpresas de colas de red o paquetes soltados. El TPU implementa todos los errores de transacción "normales". Realiza la verificación de firmas, puede informar de errores de cuenta en uso, y de lo contrario resulta en el contador, completo con pruebas de hash históricos.

## Pruebas de bajo nivel

### Cliente bancario para el banco

Debajo del nivel de TPU está el Banco. El Banco no realiza la verificación de firmas ni generara un ledger. El Banco es una capa conveniente para probar nuevos programas en cadena. Permite a los desarrolladores cambiar entre implementaciones nativas de programas y variantes compiladas por BPF. No hay necesidad de la característica de Transact aquí. La API del Banco es sincrónica.

## Pruebas unitarias con el tiempo de ejecución

Debajo del Banco está el tiempo de ejecución. El tiempo de ejecución es el entorno de prueba ideal para pruebas unitarias. Al enlazar estáticamente el tiempo de ejecución en una implementación nativa del programa, el desarrollador obtiene el bucle de edición más corto posible. Sin ningún enlace dinámico, los stack traces incluyen símbolos de depuración y errores del programa son directos a la solución de problemas.
