---
title: Validador
---

## Historia

Cuando comenzamos Solana, el objetivo era desarriesgar nuestras reclamaciones TPS. Sabíamos que entre el control de concurrencia optimista y las franjas de líderes suficientemente largas, ese consenso PoS no era el mayor riesgo para el TPS. Fue la verificación de la firma basada en GPU, el pipeline de software y la banca concurrente. Entonces la TPU nació. Después de completar 100k TPS, dividimos el equipo en un grupo trabajando hacia 710k TPS y otro para dar cuerpo al pipeline del validador. Por lo tanto, la TVU nació. La arquitectura actual es una consecuencia del desarrollo incremental con ese orden y prioridades del proyecto. No es un reflejo de lo que nunca creímos que era la sección transversal más elegante desde el punto de vista técnico de esas tecnologías. En el contexto de la rotación de líderes, la fuerte distinción entre liderar y validar es borrosa.

## Diferencia entre validar y liderar

La diferencia fundamental entre los pipelines es cuando el PoH está presente. En un líder, procesamos transacciones, eliminamos las malas, y luego etiquetamos el resultado con un hash PoH. En el validador, verificamos ese hash, lo pelamos y procesamos las transacciones exactamente igual. La única diferencia es que si un validador ve una mala transacción, no puede simplemente eliminarlo como lo hace el líder, porque eso haría que el hash de PoH cambiara. En su lugar, rechaza todo el bloque. La otra diferencia entre los pipelines es lo que sucede _después de_ banca. El líder transmite entradas a los validadores downstream mientras que el validador ya lo habrá hecho en RetransmitStage, que es una optimización del tiempo de confirmación. El canal de validación, por otro lado, tiene un último paso. Cada vez que termina de procesar un bloque, necesita pesar cualquier fork que esté observando, posiblemente emite un voto, y así, resetea su hash PoH al hash de bloque sobre el que acaba de votar.

## Diseño propuesto

Desenvolvemos las muchas capas de abstracción y construimos un único pipeline que puede activar el modo líder cuando el ID del validador aparezca en el horario del líder.

![Diagrama de bloque de validadores](/img/validator-proposal.svg)

## Cambios notables

- Aumentar FetchStage y BroadcastStage fuera de TPU
- BankForks renombrados a Banktree
- TPU se mueve a una nueva caja libre de socket llamada solana-tpu.
- La etapa bancaria de TPU absorbe ReplayStage
- El TVU desaparece
- Nueva RepairStage absorbe la Etapa de Obtención Fragmentada y solicitudes de reparación
- El servicio RPC JSON es opcional - utilizado para depuración. En su lugar, debería ser parte de un ejecutable `solana-blockstreamer` separado.
- Nuevos absorbos multicastStage retransmiten parte de RetransmitStage
- MulticastStage despues de Blockstore
