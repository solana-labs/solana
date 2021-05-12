---
title: Datos del Clúster de Sysvar
---

Solana expone una variedad de datos del estado del clúster a los programas a través de [`sysvar`](terminology.md#sysvar) cuentas. Estas cuentas se rellenan en las direcciones conocidas publicadas junto con los diseños de las cuentas en el [`programa Solana` crate](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/sysvar/index.html), y que se describen a continuación.

Para incluir los datos sysvar en las operaciones del programa, pase la dirección de la cuenta sysvar en la lista de cuentas de una operación. La cuenta se puede leer en su procesador de instrucciones como cualquier otra cuenta. Access to sysvars accounts is always _readonly_.

## Reloj

La sysvar Clock contiene datos sobre la hora del cluster, incluyendo la ranura actual, la época y la marca de tiempo Unix estimada del reloj de pared. Se actualiza cada ranura.

- Dirección: `SysvarC1ock111111111111111111111111111111111111111111111111111111`
- Diseño: [Reloj](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/clock/struct.Clock.html)
- Campos:

  - `slot`: la ranura actual
  - `epoch_start_timestamp`: la marca de tiempo Unix de la primera ranura de esta época. En la primera ranura de una época, esta marca de tiempo es idéntica a `unix_timestamp` (abajo).
  - `epoch`: la época actual
  - `programa_líder_época`: la época más reciente para la que ya se ha generado el programa líder
  - `unix_timestamp`: la marca de tiempo Unix de esta ranura.

  Cada ranura tiene una duración estimada basada en la Prueba de Historia. Pero en realidad, las ranuras pueden transcurrir más rápido y más lento que esta estimación. Como resultado, se genera la marca de tiempo de Unix de una ranura basada en la entrada de oráculo de los validadores de votación. Esta marca de tiempo se calcula como la mediana ponderada del stake de marca de tiempo proporcionadas por los votos, limitada por el tiempo esperado transcurrido desde el inicio de la época.

  De forma más explícita: para cada ranura, la marca de tiempo de la votación más reciente proporcionada por cada validador se utiliza para generar una estimación de la marca de tiempo para la ranura actual (se supone que las ranuras transcurridas desde la marca de tiempo de la votación son Bank::ns_per_slot). Cada estimación de la marca de tiempo está asociada con el stake delegado a esa cuenta de voto para crear una distribución de marcas de tiempo por stake. La mediana de la marca de tiempo se utiliza como `unix_timestamp`, a menos que el tiempo transcurrido desde la `epoch_start_timestamp` se haya desviado del tiempo transcurrido esperado en más de un 25%.

## Calendario época

The EpochSchedule sysvar contains epoch scheduling constants that are set in genesis, and enables calculating the number of slots in a given epoch, the epoch for a given slot, etc. (Note: the epoch schedule is distinct from the [`leader schedule`](terminology.md#leader-schedule))

- Dirección: `SysvarEpochSchedu1e111111111111111111111111`
- Diseño: [EpochSchedule](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/epoch_schedule/struct.EpochSchedule.html)

## Comisiones

La tarifa sysvar contiene la calculadora de comisiones para el slot actual. Se actualiza cada slot, basado en el gobernador del tipo de comisión.

- Dirección:`SysvarFees111111111111111111111111111111111`
- [Comisiones](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/sysvar/fees/struct.Fees.html)

## Instrucciones

La instrucciones sysvar contiene las instrucciones serializadas en un mensaje mientras ese mensaje está siendo procesado. Esto permite que las instrucciones del programa hagan referencia a otras instrucciones en la misma transacción. Lea más información sobre [instrucción introspección](implemented-proposals/instruction_introspection.md).

- Dirección: `Sysvar1nstructions1111111111111111111111111`
- Diseño: [Instructiones](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/sysvar/instructions/struct.Instructions.html)

## Blocks de Hash recientes

La sysvar RecentBlockhashes contiene los bloqueos recientes activos, así como sus calculadoras de comisiones asociadas. Se actualiza cada ranura.

- Dirección: `SysvarRecentB1ockHashes11111111111111111111`
- Diseño: [RecentBlockhashes](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/sysvar/recent_blockhashes/struct.RecentBlockhashes.html)

## Renta

El servicio de alquiler sysvar contiene la tarifa de alquiler. Actualmente, la tasa es estática y establecida en el génesis. El porcentaje de quemado de alquiler se modifica mediante la activación manual de características.

- Dirección: `SysvarRent111111111111111111111111111111111`
- Diseño: [Rent](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/rent/struct.Rent.html)

## SlotHashes

El sysvar SlotHashes contiene los hashes más recientes de los bancos madre de la ranura. Se actualiza cada ranura.

- Dirección: `SysvarS1otHashes111111111111111111111111111`
- Diseño: [SlotHashes](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/slot_hashes/struct.SlotHashes.html)

## SlotHistory

El sysvar SlotHistory contiene un vector de bits de las ranuras presentes en la última época. Se actualiza cada ranura.

- Dirección: `SysvarS1otHistory11111111111111111111111111`
- Diseño: [SlotHistory](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/slot_history/struct.SlotHistory.html)

## Historial de Stake

La sysvar Historial de stake contiene el historial de activaciones y desactivaciones de stake en todo el cluster y desactivaciones por época. Se actualiza al principio de cada época.

- Dirección: `SysvarStakeHistory1111111111111111111111111`
- Diseño: [Historial de Stake](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/stake_history/struct.StakeHistory.html)
