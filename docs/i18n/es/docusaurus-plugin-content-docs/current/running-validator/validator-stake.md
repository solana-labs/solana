---
title: Staking
---

**Por defecto, tu validador no tendrá ningun stake.** Esto significa que será inelegible para convertirse en líder.

## Seguimiento de la puesta al día

Para delegar stake, primero asegúrese de que su validador se está ejecutando y esta al dia con el clúster. Puede tardar algún tiempo en ponerse al día después de que el validador arranque. Utilice el comando `catchup` para supervisar su validador a través de este proceso:

```bash
solana catchup ~/validator-keypair.json
```

Hasta que su validador no se haya puesto al día, no podrá votar con éxito y no se le podrá delegar el stake.

También si encuentras que la ranura del clúster avanza más rápido que la tuya, es probable que nunca te pongas al día. Esto normalmente implica algún tipo de problema de red entre el validador y el resto del clúster.

## Crear keypair de stake

Si aún no lo ha hecho, cree un keypair de staking. Si has completado este paso, deberías ver el "validator-stake-keypair.json" en el directorio de tiempo de ejecución de Solana.

```bash
keygen nuevo -o ~/validator-stake-keypair.json
```

## Delegar tu Stake

Ahora delega 1 SOL a tu validador creando primero tu cuenta de stake:

```bash
solana create-stake-account ~/validator-stake-keypair.json 1
```

y luego delegar ese stake a tu validador:

```bash
participaciones delegadas ~/validator-stake-keypair.json ~/vote-account-keypair.json
```

> No deleges tu SOL restante, ya que tu validador usará esas fichas para votar.

Los stakes pueden ser redelegados a otro nodo en cualquier momento con el mismo comando, pero solo se permite una re-delegación por época:

```bash
participaciones delegadas ~/validator-stake-keypair.json ~/some-other-vote-account-keypair.json
```

Asumiendo que el nodo está votando, ahora estás en marcha y generando las recompensas del validador. Las recompensas se pagan automáticamente en los límites de las épocas.

Las recompensas ganadas se dividen entre su cuenta de stake y la cuenta de votos según la tasa de comisión establecida en la cuenta de votos. Las recompensas solo se pueden obtener mientras el validador esté activo y en ejecución. Además, una vez en stake, el validador se convierte en una parte importante de la red. Para eliminar de forma segura un validador de la red, primero desactive su stake.

Al final de cada ranura, se espera que un validador envíe una transacción de voto. Estas transacciones de voto son pagadas por lamports desde la cuenta de identidad de un validador.

Esta es una transacción normal por lo que se aplicará la tarifa estándar de la transacción. El rango de comisiones de transacción está definido por el bloque génesis. La comisión actual fluctuará en función de la carga de la transacción. Puede determinar la cuota actual a través de la [API RPC “getRecentBlockhash”](developing/clients/jsonrpc-api.md#getrecentblockhash) antes de enviar una transacción.

Obtenga más información sobre [las comisiones de transacción aquí](../implemented-proposals/transaction-fees.md).

## Atención de stake del Validador

Para combatir varios ataques al consenso, las nuevas delegaciones de stakeestán sujetas a un [periodo de calentamiento](/staking/stake-accounts#delegation-warmup-and-cooldown).

Supervisar la participación de un validador durante el warmup por:

- Ver su cuenta de voto:`solana vote-account ~/vote-account-keypair.json` Esto muestra el estado actual de todos los votos que el validador ha enviado a la red.
- Ver tu cuenta de stake, la preferencia de delegación y los detalles de tu stake:`solana stake-account ~/validator-stake-keypair.json`
- `validadores de solana` muestra el stake activo actual de todos los validadores, incluyendo el tuyo
- `historial de stake de solana` muestra el historial del calentamiento y enfriamiento en las épocas recientes
- Busca mensajes de registro en tu validador indicando tu próxima ranura de líder: `[2019-09-27T20:16:00.319721164Z INFO solana_core::replay_stage] <VALIDATOR_IDENTITY_PUBKEY> votó y restableció PoH a altura de tick ####. Mi siguiente ranura de líder es ####`
- Una vez que tu stake se haya encendido, verás un saldo de stake en la lista para tu validador al ejecutar `validadores de solana`

## Supervisar a su validador

Confirma que tu validador se convierte en un [líder](../terminology.md#leader)

- Después de que su validador se ponga al día, utilice el comando `solana balance` para supervisar las ganancias a medida que su validador es seleccionado como líder y recoge las tasas de transacción
- Los nodos Solana ofrecen una serie de métodos útiles JSON-RPC para devolver información sobre la red y la participación de su validador. Haga una solicitud usando curl \(u otro cliente http de su elección\), especificando el método deseado en los datos con formato JSON-RPC. Por ejemplo:

```bash
  // Solicitud
  curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getEpochInfo"}' http://localhost:8899

  // Resultado
  {"jsonrpc":"2.0","result":{"epoch":3,"slotIndex":126,"slotsInEpoch":256},"id":1}
```

Métodos JSON-RPC útiles:

- `getEpochInfo`[Un epoch](../terminology.md#epoch) es el momento, p.e. número de espacios [](../terminology.md#slot), para los cuales un horario [de líder](../terminology.md#leader-schedule) es válido. Esto le dirá lo que es el epicentro actual y lo lejos que está el cluster.
- `getVoteAccounts` Esto te dirá cuánto stake activo tiene tu validador. Un % del stake del validador se activa en un límite de época. Puedes aprender más sobre hacer stake en Solana [aquí](../cluster/stake-delegation-and-rewards.md).
- `getLeaderSchedule` En cualquier momento dado, la red espera que sólo un validador produzca entradas de ledger. El validador [seleccionado actualmente para producir entradas de ledger](../cluster/leader-rotation.md#leader-rotation) se llama el "líder". Esto devolverá el programa completo de líder \(sobre una base de ranura por ranura\) para el stake activado, el pubkey de identidad aparecerá 1 o más veces aquí.

## Desactivando stake

Antes de separar a tu validador del clúster, deberías desactivar el stake que fue previamente delegado ejecutando:

```bash
solana deactivate-stake-~/validator-stake-keypair.json
```

El stake no se desactiva inmediatamente y en su lugar se enfria de manera similar como el stake "se calienta". Tu validador debe permanecer unido al clúster mientras que el stake se enfria. Mientras se enfría, su stake seguirá ganando recompensas. Sólo después de enfriar el stake es seguro desactivar tu validador o retirarlo de la red. El tiempo de enfriamiento puede tardar varias épocas en completarse, dependiendo del stake activo y el tamaño de tu stake.

Tenga en cuenta que una cuenta de stake sólo puede utilizarse una vez, así que después de la desactivación, usa el comando `de retiro` de Ci para recuperar los lamports previamente en stake.
