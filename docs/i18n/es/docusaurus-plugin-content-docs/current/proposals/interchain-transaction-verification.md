---
title: Verificación de transacciones intercadenas
---

## Problema

Las aplicaciones intercadenas no son nuevas en el ecosistema de activos digitales; De hecho, incluso los intercambios centralizados más pequeños todavía ponen de relieve categóricamente todas las aplicaciones de una sola cadena reunidas en términos de usuarios y volumen. Mantienen valoraciones masivas y han pasado años optimizando eficazmente sus productos básicos para una amplia gama de usuarios finales. Sin embargo, sus operaciones básicas se centran en mecanismos que requieren que sus usuarios confíen en ellos de manera secundaria. típicamente con poco o ningún recurso o protección contra pérdidas accidentales. Esto ha llevado a fracturar el ecosistema de activos digitales más amplio a lo largo de las líneas de red porque las soluciones de interoperabilidad típicamente:

- Son técnicamente complejas de aplicar en su totalidad
- Crear estructuras de incentivos inestables a escala de red
- Requiere una cooperación consistente y de alto nivel entre las partes interesadas

## Solución propuesta

Simple Payment Verification \(SPV\) es un término genérico para un rango de diferentes metodologías utilizadas por clientes ligeros en la mayoría de las principales redes de blockchain para verificar aspectos del estado de la red sin la carga de almacenar y mantener la cadena misma. En la mayoría de los casos esto significa confiar en una forma de árbol hash para proporcionar una prueba de la presencia de una transacción determinada en un bloque determinado comparando con un hash de raíz en la cabecera o equivalente de ese bloque. Esto permite a un cliente ligero o a un monedero alcanzar un nivel probabilístico de certeza sobre los eventos en la cadena por sí mismo, con un mínimo de confianza requerida con respecto a los nodos de la red.

Tradicionalmente, el proceso de ensamblaje y validación de estas pruebas se lleva a cabo a partir de la cadena por nodos, billeteras, u otros clientes, pero también ofrece un mecanismo potencial para la verificación entre cadenas de estado. Sin embargo, al trasladar la capacidad de validar las pruebas de SPV en la cadena como un contrato inteligente al tiempo que se aprovechan las propiedades de archivo inherentes a la cadena de bloques, es posible construir un sistema para detectar y verificar programáticamente las transacciones en otras redes sin la participación de ningún tipo de oráculo de confianza o mecanismo de consenso complejo de varias etapas. Este concepto es ampliamente generalizable a cualquier red con un mecanismo de SPV e incluso puede ser operado bilateralmente en otras plataformas de contratos inteligentes, abriendo la posibilidad de una transferencia de valor barata, rápida y entre cadenas sin depender de garantías, hashlocks o intermediarios de confianza.

Optar por aprovechar los mecanismos bien establecidos y estables en su desarrollo que ya son comunes a todas las principales cadenas de bloques permite que las soluciones de interoperabilidad basadas en SPV sean mucho más sencillas que los enfoques orquestados de varias etapas. Como parte de esto, prescinden de la necesidad de estándares de comunicación de cadena cruzada ampliamente acordados y de las grandes organizaciones multigrupales que los escriben en favor de un conjunto de servicios discretos basados en contratos que pueden ser fácilmente utilizados por contratos de llamada a través de un formato de abstracción común. Esto sentará las bases para una amplia gama de aplicaciones y contratos capaces de interactuar a través del variegado y cada ecosistema de plataformas en crecimiento.

## Terminología

Programa SPV - Interfaz orientada al cliente para el sistema SPV intercadena, gestiona los roles de participante. SPV Engine - Valida pruebas de transacción, subconjunto del programa SPV. Cliente - La persona que llama al programa SPV, típicamente otro contrato de solana. Prover - Parte que genera pruebas para las transacciones y las presenta al Programa SPV. Prueba de transacción - Creado por Provers, contiene una referencia a prueba de merkle, transacción y encabezado de bloques. Merkle Proof - Prueba SPV básica que valida la presencia de una transacción en un bloque determinado. Cabecera de bloque - Representa los parámetros básicos y la posición relativa de un bloque determinado. Solicitud de prueba - Un pedido realizado por un cliente para la verificación de transacción\(s\) por provers. Tienda de cabeceras - Una estructura de datos para almacenar y referenciar rangos de cabeceras de bloque en pruebas. Solicitud de cliente - Transacción desde el cliente al programa SPV para activar la creación de una solicitud de prueba. Subcuenta - Una cuenta Solana propiedad de otra cuenta contractual, sin su propia clave privada.

## Servicio

Los programas de SPV funcionan como contratos desplegados en la red Solana y mantienen un tipo de mercado público para las pruebas de SPV que permite a cualquier parte presentar ambas solicitudes de pruebas así como pruebas para su verificación en respuesta a las solicitudes. Habrá múltiples instancias de programa SPV activas en cualquier momento dado al menos una para cada red externa conectada y potencialmente múltiples instancias por red. Las instancias del programa SPV serán relativamente consistentes en su API de alto nivel y conjuntos de características con alguna variación entre plataformas de divisas \(Bitcoin, Litecoin\) y plataformas de contratos inteligentes debido al potencial de verificación de cambios en el estado de la red más allá de simples transacciones. En todos los casos, independientemente de la red, el programa SPV se basa en un componente interno llamado motor SPV para proporcionar una verificación sin estado de las pruebas reales de SPV sobre las que se construyen las características y api del cliente de más alto nivel. El motor SPV requiere una implementación específica de la red, pero permite una fácil extensión del mayor ecosistema intercadena por cualquier equipo que decida llevar a cabo esa implementación y dejarla caer en el programa estándar SPV para su implementación.

Para propósitos de solicitud de prueba, el solicitante se denomina cliente del programa, que en la mayoría de los casos si no todos serán otro contrato de Solana. El cliente puede elegir enviar una solicitud perteneciente a una transacción específica o incluir un filtro más amplio que se pueda aplicar a cualquiera de los parámetros de una transacción incluyendo sus entradas, salidas y cantidad. Por ejemplo, Un cliente podría enviar una solicitud para cualquier transacción enviada desde una dirección A dada para dirección B con la cantidad X después de un cierto tiempo. Esta estructura puede ser usada en un rango de aplicaciones, tales como verificar un pago específico previsto en el caso de un intercambio atómico o detectar el movimiento de activos colaterales de garantía para un préstamo.

Siguiendo el envío de una solicitud de cliente, asumiendo que se validó con éxito, una cuenta de solicitud de prueba es creada por el programa SPV para seguir el progreso de la solicitud. Proveedores de usar la cuenta para especificar la solicitud que tienen la intención de rellenar las pruebas que envían para su validación, En ese momento el programa SPV valida esas pruebas y si lo hace con éxito, las guarda en los datos de la cuenta de la cuenta. Los clientes pueden monitorear el estado de sus solicitudes y ver cualquier transacción aplicable junto con sus pruebas consultando los datos de cuenta de la cuenta solicitada. En futuras iteraciones apoyadas por Solana, este proceso se simplificará mediante contratos de publicación de eventos en lugar de requerir un proceso de estilo de votación como se describe.

## Implementación

El mecanismo SPV de Solana Inter-chain consiste en los siguientes componentes y participantes:

### Motor SPV

Un contrato desplegado en Solana que verifica sin estado las pruebas SPV para la persona que llama. Toma como argumentos para la validación:

- Una prueba de SPV en el formato correcto del blockchain asociado al programa
- Referencia\(s\) a las cabeceras de bloques relevantes para comparar esa prueba contra
- Los parámetros necesarios de la transacción para verificar

  Si el comprobante en cuestión es validado con éxito, el programa SPV guarda prueba

  de esa verificación a la cuenta de solicitud, que puede ser guardada por la persona que llama a

  los datos de su cuenta o de otro modo manejados según sea necesario. Los programas SPV también exponen

  utilidades y estructuras utilizadas para la representación y validación de cabeceras,

  transacciones, hashes, etc. sobre una cadena por cadena.

### Programa SPV

Un contrato desplegado en Solana que coordina e intermedia la interacción entre Clientes y Proveedores y gestiona la validación de solicitudes, cabeceras, pruebas, etc. Es el principal punto de acceso de los contratos del Cliente para acceder a la intercadena. Mecanismo SPV. Ofrece las siguientes características principales:

- Enviar solicitud de prueba - permite al cliente presentar una solicitud de prueba específica o conjunto de pruebas
- Cancelar la solicitud de prueba - permite al cliente invalidar una solicitud pendiente
- Llenar la Solicitud de Prueba - utilizado por los Provers para enviar para validación una prueba correspondiente a una Solicitud de Prueba dada

  El programa SPV mantiene una lista disponible públicamente de la prueba válida pendiente

  Solicita datos de su cuenta en beneficio de los Provers, que la monitorean y

  adjuntar las referencias a las solicitudes de destino con sus pruebas presentadas.

### Solicitud de prueba

Un mensaje enviado por el Cliente al motor SPV que denomina una solicitud de prueba de una transacción específica o conjunto de transacciones. Las solicitudes de prueba pueden especificar manualmente una transacción determinada por su hash o puede elegir enviar un filtro que coincida con varias transacciones o clases de transacciones. Por ejemplo, un filtro que coincida con “cualquier transacción de la dirección xxx a la dirección yy” podría utilizarse para detectar el pago de una deuda o liquidación de un intercambio intercadena. Del mismo modo, un filtro que coincida con “cualquier transacción desde la dirección xxx” podría ser utilizado por un contrato de préstamo o sintético de minting de tokens para monitorear y reaccionar a los cambios en la colateralización. Las solicitudes de prueba se envían con un honorario, que es desembolsado por el contrato del motor SPV a la Prover apropiada una vez que se valida una prueba que coincida con esa solicitud.

### Solicitar Libro

El listado público de solicitudes de prueba válidas y abiertas disponibles para los provers que deben llenar o para que los clientes cancelen. Analógico a un libro de pedidos en un intercambio, pero con un solo tipo de listado, en lugar de dos lados separados. Se almacena en los datos de la cuenta del programa SPV.

### Prueba

Una prueba de la presencia de una transacción determinada en el blockchain en cuestión. Las pruebas abarcan tanto la prueba actual de merkle como la referencia\(s\) a una cadena de cabeceras de bloque válidas. Son construidos y presentados por Provers de acuerdo con las especificaciones de las Solicitudes de Prueba disponibles públicamente alojadas en el libro de solicitud del programa SPV. Tras la validación, se guardan en los datos de la cuenta de la solicitud de prueba pertinente, que puede ser utilizado por el Cliente para monitorear el estado de la solicitud.

### Cliente

El autor de una solicitud de prueba de transacción. Los clientes serán más a menudo otros contratos como parte de aplicaciones o productos financieros específicos como préstamos, swaps, escrow, etc. El cliente en cualquier ciclo de proceso de verificación inicial envía una solicitud de cliente que comunica los parámetros y la comisión y, si se validó con éxito, resulta en la creación de una cuenta de Prueba de Solicitud por el programa SPV. El Cliente también puede enviar una Solicitud de Cancelación referenciando una Solicitud de Prueba activa con el fin de denotarla como inválida para propósitos de envío de pruebas.

### Prover

El remitente de una prueba que llena una solicitud de prueba. Los Proveedores monitorean el libro de solicitudes del programa SPV para solicitudes de prueba pendientes y generan pruebas coincidentes, que envían al programa SPV para su validación. Si el comprobante es aceptado, la cuota asociada a la Solicitud de Prueba en cuestión será abonada a la Prover. Proveedores normalmente funcionan como nodos Solana Blockstreamer que también tienen acceso a un nodo Bitcoin, que utilizan para construir pruebas y acceder a las cabeceras de bloques.

### Tienda de cabecera

Una estructura de datos basada en la cuenta utilizada para mantener los encabezados de bloque con el propósito de la inclusión en las pruebas presentadas por referencia a la cuenta de la tienda de encabezados. las tiendas de cabeceras pueden ser mantenidas por entidades independientes, ya que la validación de la cadena de cabeceras es un componente del mecanismo de validación de prueba del programa SPV. Las cuotas pagadas por Provers por Pruebas se dividen entre el remitente de la prueba de Merkle en sí y la tienda de cabeceras a la que se hace referencia en la prueba presentada. Debido a la incapacidad actual de crecer ya asignada capacidad de datos de cuenta, el caso de uso necesita una estructura de datos que puede crecer indefinidamente sin reequilibrarse. Las subcuentas son cuentas propiedad del programa SPV sin sus propias claves privadas que se utilizan para el almacenamiento asignando las cabeceras de bloqueo a los datos de su cuenta. Múltiples enfoques potenciales para la implementación del sistema de almacenamiento de cabeceras son factibles:

Almacenar cabeceras en subcuentas del programa indexadas por dirección pública:

- Cada subcuenta tiene un encabezado y tiene una clave pública que coincide con el blockhash
- Requiere el mismo número de búsquedas de datos de cuenta que las confirmaciones por verificación
- Limitar el número de confirmaciones \(15-20\) mediante el límite máximo de datos de transacción
- No hay duplicación en toda la red de cabeceras individuales

Lista vinculada de múltiples cabeceras de almacenamiento de subcuentas:

- Mantener índice secuencial de cuentas de almacenamiento, muchos encabezados por cuenta de almacenamiento
- Máximo 2 búsquedas de datos de cuenta para &gt;99,9% de verificaciones \(1 para máximo\)
- Formato de dirección de datos secuencial compacto permite cualquier número de confirmaciones y búsquedas rápidas
- Facilita las ineficiencias de duplicación en toda la red
