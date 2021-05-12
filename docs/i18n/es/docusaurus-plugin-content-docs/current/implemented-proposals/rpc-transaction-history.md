# Historial de transacciones RPC a largo plazo
Es necesario que RPC sirva al menos 6 meses de historial de transacciones.  El historial actual por orden de los días, es insuficiente para los usuarios de la corriente posterior.

6 meses de datos de transacción no pueden almacenarse prácticamente en el gestor de contabilidad rocksdb de un validador, por lo que es necesario un almacenamiento de datos externo.   El validador rocksdb ledger continuará sirviendo como la fuente principal de datos y luego volverá al almacenamiento de datos externo.

Los puntos finales RPC afectados son:
* [getFirstavailableBlock](developing/clients/jsonrpc-api.md#getfirstavailableblock)
* [getConfirmedBlock](developing/clients/jsonrpc-api.md#getconfirmedblock)
* [getConfirmedBlocks](developing/clients/jsonrpc-api.md#getconfirmedblocks)
* [getConfirmedSignaturesForAddress](developing/clients/jsonrpc-api.md#getconfirmedsignaturesforaddress)
* [getConfirmedTransaction](developing/clients/jsonrpc-api.md#getconfirmedtransaction)
* [getSignatureStatuses](developing/clients/jsonrpc-api.md#getsignaturestatuses)

Tenga en cuenta que [getBlockTime](developing/clients/jsonrpc-api.md#getblocktime) no es compatible, ya que una vez https://github.com/solana-labs/solana/issues/10089 es arreglado entonces `getBlockTime` puede ser removido.

Algunas restricciones de diseño del sistema:
* El volumen de datos para almacenar y buscar puede saltar rápidamente a los terabytes, y es inmutable.
* El sistema debe ser lo más ligero posible para SREs.  Por ejemplo, un clúster de base de datos de SQL que requiere un SRE para monitorizar y reequilibrar continuamente nodos no es posible.
* Los datos deben ser buscables en tiempo real - las consultas en lote que toman minutos o horas para ejecutarse son inaceptables.
* Fácil de replicar los datos en todo el mundo para co-ubicarlos con los endpoints RPC que lo utilizarán.
* Interfaz con el almacén de datos externo debería ser fácil y no requerir dependiendo de las bibliotecas de código fácilmente usadas por la comunidad

Basado en estas restricciones, el producto BigTable de Google se selecciona como la tienda de datos.

## Esquema de tabla
Una instancia BigTable se utiliza para guardar todos los datos de transacción, divididos en tablas diferentes para buscar rápidamente.

Los nuevos datos pueden copiarse en la instancia en cualquier momento sin afectar a los datos existentes, y todos los datos son inmutable.  Generalmente, la expectativa es que los nuevos datos se cargarán una vez que se complete la época actual, pero no hay limitación en la frecuencia de los volcados de datos.

La limpieza de los datos antiguos es automática configurando la política de retención de datos de las tablas de instancia apropiadamente, solo desaparece.  Por lo tanto, el orden en el que se añaden los datos se vuelve importante.  Por ejemplo, si los datos de epoch N-1 se añaden después de los datos de epoch N, los datos de epoch antiguos superarán a los datos nuevos.  Sin embargo, más allá de producir _holes_ en los resultados de la consulta, este tipo de borrado desordenado no tendrá ningún efecto mal.  Ten en cuenta que este método de limpieza permite almacenar un ilimitado de datos de transacción, restringido sólo por los costos monetarios de hacerlo.

La disposición de la tabla s sólo soporta los puntos finales RPC existentes.  Los nuevos endpoints RPC en el futuro pueden requerir adiciones al esquema y potencialmente iterar más de todas las transacciones para crear los metadatos necesarios.

## Accediendo a BigTable
BigTable tiene un endpoint gRPC al que se puede acceder usando la [tónic](https://crates.io/crates/crate)] y la API de protobugs crudos, como actualmente no existe ninguna jaula de mayor nivel para BigTable.  Prácticamente esto hace que el análisis de resultados de consultas BigTable sea más complicado pero no es un problema significativo.

## Población de datos
La población en curso de datos de instancia ocurrirá en una cadencia de epoch a través del uso de un nuevo comando ` solana-ledger-tool ` que convertirá los datos de rocksdb para un rango de ranuras dado en el esquema de instancia.

El mismo proceso se ejecutará una vez, manualmente, para rellenar los datos del ledger existente.

### Mesa de bloques: `block`

Esta tabla contiene los datos del bloque comprimido para una rama determinada.

La clave de fila se genera al tomar la representación hexadecimal de 16 dígitos en minúscula de la ranura para asegurarse de que la ranura más antigua con un bloque confirmado siempre será la primera vez que las filas sean listadas.  ej. La clave de fila para la ranura 42 sería 00000000000000002a.

Los datos de fila son una estructura `StoredConfirmedBlock`.


### Tabla de Búsqueda de la dirección de la transacción: `tx-by-addr`

Esta tabla contiene las transacciones que afectan a una dirección determinada.

La clave de fila es `<base58
address>/<slot-id-one's-compliment-hex-slot-0-prefixed-to-16-digits>`.  Los datos de fila son una estructura `StoredConfirmedBlock`.

Asumir el complemento de la ranura permite listar ranuras asegura que la ranura más reciente con transacciones que afectan a una dirección siempre se muestre primero.

Las direcciones Sysvar no están indexadas.  Sin embargo los programas de uso frecuente como Voto o Sistema son, y probablemente tendrán una fila para cada ranura confirmada.

### Mesa de búsqueda de firma de transacción: `tx`

Esta tabla mapea una firma de transacción a su bloque confirmado, e índice dentro de ese bloque.

La clave de registro es la firma de transacción codificada en base 58. Los datos de fila son una estructura `StoredConfirmedBlock`.
