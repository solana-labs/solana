---
title: Almacenamiento de cuenta persistente
---

## Almacenamiento de cuenta persistente

The set of accounts represent the current computed state of all the transactions that have been processed by a validator. Cada validador necesita mantener este conjunto completo. Each block that is proposed by the network represents a change to this set, and since each block is a potential rollback point, the changes need to be reversible.

Almacenamiento persistente como NVMEs son entre 20 y 40 veces más baratas que DDR. The problem with persistent storage is that write and read performance is much slower than DDR. Care must be taken in how data is read or written to. Both reads and writes can be split between multiple storage drives and accessed in parallel. This design proposes a data structure that allows for concurrent reads and concurrent writes of storage. Writes are optimized by using an AppendVec data structure, which allows a single writer to append while allowing access to many concurrent readers. The accounts index maintains a pointer to a spot where the account was appended to every fork, thus removing the need for explicit checkpointing of state.

## AppendVec

AppendVec es una estructura de datos que permite lecturas aleatorias simultáneas con un solo escritor de anexo. Crecer o redimensionar la capacidad del AppendVec requiere acceso exclusivo. Esto se implementa con un `offset` atómico, que se actualiza al final de un anexo completo.

La memoria subyacente de un AppendVec es un archivo mapeado en memoria. Los archivos mapeados por memoria permiten un acceso rápido al azar y la paginación es manejada por el sistema operativo.

## Índice de cuenta

El índice de cuenta está diseñado para soportar un solo índice para todas las cuentas actualmente bifurcadas.

```text
type AppendVecId = usize;

type Fork = u64;

struct AccountMap(Hashmap<Fork, (AppendVecId, u64)>);

type AccountIndex = HashMap<Pubkey, AccountMap>;
```

El índice es un mapa de cuenta Pubkeys a un mapa de bifurcaciones y la ubicación de los datos de la cuenta en un AppendVec. Para obtener la versión de una cuenta de una bifurcacion en específico:

```text
/// Carga la cuenta para el pubkey.
/// Esta función cargará la cuenta desde la bifurcación especificada, volviendo a los padres de la bifurcación
/// * fork: una instancia de cuentas virtuales, con clave de Fork.  Las cuentas mantienen un seguimiento de sus padres con bifurcaciones,
/// la tienda persistente
/// * pubkey - La clave pública de la cuenta.
pub fn load_slow(&self, id: Fork, pubkey: &Pubkey) -> Opción<&Cuenta>
```

La lectura está satisfecha al apuntar a una ubicación mapeada en la memoria en el `AppendVecId` en el desplazamiento almacenado. Una referencia puede ser devuelta sin una copia.

### Bifurcaciones de root

[Tower BFT](tower-bft.md) eventualmente selecciona una bifurcación como bifurcación raíz y la bifurcación es aplastada. Una bifurcación aplastada / raíz no se puede deshacer.

Cuando una bifurcación es aplastada, todas las cuentas en sus padres que aún no están presentes en la bifurcación son arrastradas a la bifurcación actualizando los índices. Las cuentas con saldo cero en la bifurcación aplastada se eliminan de la bifurcación actualizando los índices.

Una cuenta puede ser _garbage-collected_ cuando el aplastamiento la hace inaccesible.

Existen tres opciones posibles:

- Mantener un HashSet de bifurcaciones de root. Se espera que se cree una cada segundo. Todo el árbol puede ser recolectado más tarde. Alternativamente, si cada bifurcación mantiene un recuento de cuentas de referencia, la recolección de basura podría ocurrir cada vez que se actualice una ubicación de índice.
- Retire las bifurcaciones podadas del índice. Cualquier bifurcación restante menor en número que la raíz se puede considerar root.
- Escanea el índice, migra cualquier raíz antigua a la nueva. Cualquier bifurcación restante inferior a la nueva raíz se puede eliminar más tarde.

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

- Las escrituras sólo de anexos son rápidas. SSDs y NVMEs, así como todas las estructuras de datos del núcleo a nivel del sistema operativo, permitir que los anexos se ejecuten tan rápido como PCI o NVMe de ancho de banda permitirá \(2,700 MB/s\).
- Cada repetición y hilo bancario escribe simultáneamente en su propio AppendVec.
- Cada AppendVec podría alojarse en una NVMe separada.
- Cada hilo de repetición y banca tiene acceso de lectura simultáneo a todos los AppendVecs sin bloquear escrituras.
- El índice requiere un bloqueo de escritura exclusivo para escrituras. El rendimiento de un hilo para las actualizaciones de HashMap es de 10m por segundo.
- Las etapas de Banca y Replay deben usar 32 hilos por NVMe. NVMes tiene un rendimiento óptimo con 32 lectores o escritores simultáneos.
