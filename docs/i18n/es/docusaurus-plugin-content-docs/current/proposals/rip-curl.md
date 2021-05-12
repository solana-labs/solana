# RiP Curl: RPC de baja latencia y orientado a transacciones

## Problema

La implementación inicial de la RPC de Solana fue creada con el propósito de permitir a los usuarios confirmar las transacciones que acababan de ser enviadas recientemente al clúster. Fue diseñado teniendo en cuenta el uso de memoria tal que cualquier validador debería ser capaz de soportar la API sin preocuparse por los ataques de DoS.

Más adelante en la línea, se volvió deseable usar esa misma API para apoyar al explorador de Solana. El diseño original sólo admite minutos de historia, así que lo cambiamos para almacenar estados de transacción en una instancia local de RocksDB y ofrecer días de historia. Luego lo ampliamos a 6 meses a través de BigTable.

Con cada modificación, la API se volvió más adecuada para aplicaciones que sirven contenido estático y menos atractiva para el procesamiento de transacciones. Los clientes preguntan por el estado de la transacción en lugar de ser notificados, dando la falsa impresión de que los tiempos de confirmación son mayores. Además, lo que los clientes pueden sondear es limitado, lo que les impide tomar decisiones razonables en tiempo real, como reconocer que una transacción está confirmada tan pronto como determinados validadores de confianza la voten.

## Solución propuesta

Una API de streaming orientada a las transacciones y fácil de usar, construida alrededor del ReplayStage del validador.

Mejora la experiencia del cliente:

* Soporte de conexiones directamente desde aplicaciones de WebAssembly.
* Los clientes pueden ser notificados del progreso de la confirmación en tiempo real, incluyendo votos y peso del stake de votantes.
* Los clientes pueden ser notificados cuando el fork más pesado cambia, si afecta al recuento de confirmación de transacciones.

Fácil para que los validadores soporten:

* Cada validador soporta algún número de conexiones simultáneas y de lo contrario no tiene restricciones significativas de recursos.
* El estado de la transacción nunca se almacena en memoria y no se puede encuestar.
* Las firmas sólo se almacenan en memoria hasta que el nivel de compromiso deseado o hasta que el blockhash caduque, que nunca es más tarde.

Cómo funciona:

1. El cliente se conecta a un validador usando un canal de comunicación confiable, como un socket web.
2. El validador registra la firma con ReplayStage.
3. El validador envía la transacción a la cadena del Golfo y vuelve a intentar todos los forks conocidos hasta que el blockhash expire (no hasta que la transacción sea aceptada en sólo el fork más pesado). Si el blockhash caduca, la firma no está registrada, se notificará al cliente y se cerrará la conexión.
4. A medida que ReplayStage detecta eventos que afectan el estado de la transacción, notifica al cliente en tiempo real.
5. Después de confirmar que la transacción es rooteada (`CommitmentLevel::Max`), la firma no está registrada y el servidor cierra el canal original.
