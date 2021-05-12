---
title: Almacenamiento de cuenta persistente
---

## Almacenamiento de cuenta persistente

El conjunto de cuentas representa el estado computado actual de todas las transacciones que han sido procesadas por un validador. Cada validador necesita mantener este conjunto completo. Cada bloque propuesto por la red representa un cambio en este conjunto, y dado que cada bloque es un punto de cancelación potencial, los cambios deben ser reversibles.

Almacenamiento persistente como NVMEs son entre 20 y 40 veces más baratas que DDR. El problema con el almacenamiento persistente es que el rendimiento de escritura y lectura es mucho más lento que el DDR y hay que tener cuidado en cómo se lee o escribe los datos. Tanto las lecturas como las escrituras se pueden dividir entre múltiples unidades de almacenamiento y acceder en paralelo. Este diseño propone una estructura de datos que permite lecturas simultáneas y escrituras simultáneas de almacenamiento. Las escrituras están optimizadas usando una estructura de datos AppendVec, que permite a un solo escritor añadir mientras permite el acceso a muchos lectores simultáneos. El índice de cuentas mantiene un puntero a un lugar donde la cuenta fue añadida a cada bifurcación, eliminando así la necesidad de un control explícito del estado.

## AppendVec

AppendVec es una estructura de datos que permite lecturas aleatorias simultáneas con un solo escritor de anexo. Crecer o redimensionar la capacidad del AppendVec requiere acceso exclusivo. Esto se implementa con un ` offset ` atómico, que se actualiza al final de un anexo completo.

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

## Escrituras solo para anexar

Todas las actualizaciones a los clientes ocurren como actualizaciones de sólo agregar. Para cada actualización de cuenta, se almacena una nueva versión en el AppendVec.

Es posible optimizar las actualizaciones dentro de una sola bifurcación devolviendo una referencia mutable a una cuenta ya almacenada en una bifurcación. El Banco ya rastrea el acceso concurrente de cuentas y garantiza que una escritura en una bifurcación de cuenta específica no será simultánea con una lectura en una cuenta en esa bifurcación. Para apoyar esta operación, AppendVec debe implementar esta función:

```text
fn get_mut(&self index: u64) -> &mut T;
```

Esta API permite el acceso mutable concurrente a una región de memoria en `index`. Depende del Banco para garantizar el acceso exclusivo a ese índice.

## Recolección de basura

A medida que se actualizan las cuentas, se mueven al final del AppendVec. Una vez que se haya agotado la capacidad, se puede crear un nuevo AppendVec y almacenar las actualizaciones allí. Eventualmente las referencias a un AppendVec anterior desaparecerán porque todas las cuentas han sido actualizadas, y el AppendVec antiguo puede ser eliminado.

Para acelerar este proceso, es posible mover cuentas que no han sido actualizadas recientemente al frente de un nuevo AppendVec. Esta forma de recolección de basura se puede hacer sin necesidad de bloqueos exclusivos para ninguna de las estructuras de datos excepto para la actualización del índice.

La implementación inicial para la recolección de basura es que una vez que todas las cuentas de un AppendVec se convierten en versiones obsoletas, se reutiliza. Las cuentas no se actualizan o se mueven una vez adjuntadas.

## Recuperación de índices

Cada hilo bancario tiene acceso exclusivo a las cuentas durante el anexo, ya que las cuentas bloqueadas no pueden ser liberadas hasta que los datos sean confirmados. Pero no hay un orden explícito de escrituras entre los archivos AppendVec separados. Para crear una orden, el índice mantiene un contador atómico de versiones de escritura. Cada anexo al AppendVec registra el número de versión de escritura del índice para ese anexo en la entrada de la Cuenta en el AppendVec.

Para recuperar el índice, todos los archivos AppendVec pueden ser leídos en cualquier orden, y la última versión de escritura para cada bifurcación debe ser almacenada en el índice.

## Instantáneas

Para hacer una instantánea, los archivos mapeados en memoria del AppendVec deben ser volcados al disco. El índice también se puede escribir en el disco.

## Rendimiento

- Las escrituras sólo de anexos son rápidas. SSDs y NVMEs, así como todas las estructuras de datos del núcleo a nivel del sistema operativo, permitir que los anexos se ejecuten tan rápido como PCI o NVMe de ancho de banda permitirá \(2,700 MB/s\).
- Cada repetición y hilo bancario escribe simultáneamente en su propio AppendVec.
- Cada AppendVec podría alojarse en una NVMe separada.
- Cada hilo de repetición y banca tiene acceso de lectura simultáneo a todos los AppendVecs sin bloquear escrituras.
- El índice requiere un bloqueo de escritura exclusivo para escrituras. El rendimiento de un hilo para las actualizaciones de HashMap es de 10m por segundo.
- Las etapas de Banca y Replay deben usar 32 hilos por NVMe. NVMes tiene un rendimiento óptimo con 32 lectores o escritores simultáneos.
