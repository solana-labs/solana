## Reiniciando un clúster

### Paso 1. Identificar el slot en el que se reiniciará el clúster

La ranura más alta confirmada con optimismo es la mejor ranura para empezar, que se puede encontrar buscando [este](https://github.com/solana-labs/solana/blob/0264147d42d506fb888f5c4c021a998e231a3e74/core/src/optimistic_confirmation_verifier.rs#L71) punto de datos de métrica. De lo contrario, use la última root.

Llame a esta ranura `SLOT_X`

### Paso 2. Detener validador(es)

### Paso 3. Opcionalmente instalar la nueva versión de solana

### Paso 4. Crea un nuevo snapshot para la ranura `SLOT_X` con un fork duro en la ranura `SLOT_X`

```bash
$ solana-ledger-tool -l ledger create-snapshot SLOT_X ledger --hard-fork SLOT_X
```

El directorio ledger ahora debe contener el nuevo snapshot. `solana-ledger-tool create-snapshot` también mostrará la nueva versión fragmentada, y el valor hash bancario, llame a esto NEW_SHRED_VERSION y NEW_BANK_HASH respectivamente.

Ajuste los argumentos de su validador:

```bash
 --wait-for-supermajority SLOT_X
 --expected-bank-hash NOW_BANK_HASH
```

Luego reinicie el validador.

Confirme con el registro que el validador inició y ahora está en un patrón de retención en `SLOT_X`, esperando una súper mayoría.

### Paso 5. Anunciar el reinicio en Discord:

Publica algo como lo siguiente en #anuncios (ajustando el texto según sea apropiado):

> Hola @Validators,
>
> Hemos lanzado la versión 1.1.12 y estamos listos para hacer una copia de seguridad de testnet de nuevo.
>
> Pasos: 1. Instalar la versión v1.1.12: https://github.com/solana-labs/solana/releases/tag/v1.1.12 2. a. Método preferido, empieza desde tu ledger local con:
>
> ```bash
> solana-validator
>   --wait-for-supermajority SLOT_X # <-- ¡NUEVO! IMPORTANT! REMOVE AFTER THIS RESTART
>   --expected-bank-hash NEW_BANK_HASH  # <-- NEW! IMPORTANT! REMOVE AFTER THIS RESTART
>   --hard-fork SLOT_X                  # <-- NEW! IMPORTANT! REMOVE AFTER THIS RESTART
>   --no-snapshot-fetch                 # <-- NEW! IMPORTANT! REMOVE AFTER THIS RESTART
>   --entrypoint entrypoint.testnet.solana.com:8001
>   --trusted-validator 5D1fNXzvv5NjV1ysLjirC4WY92RNsVH18vjmcszZd8on
>   --expected-genesis-hash 4uhcVJyU9pJkvQyS88uRDiswHXSCkY3zQawwpjk2NsNY
>   --no-untrusted-rpc
>   --limit-ledger-size
>   ...                                # <-- your other --identity/--vote-account/etc arguments
> ```

````

b. Si su validador no tiene ledger hasta la ranura SLOT_X o si ha eliminado su ledger, en su lugar descargue una snapshot con:

```bash
solana-validator
  --wait-for-supermajority SLOT_X     # <-- NEW! IMPORTANT! REMOVE AFTER THIS RESTART
  --expected-bank-hash NEW_BANK_HASH  # <-- NEW! IMPORTANT! REMOVE AFTER THIS RESTART
  --entrypoint entrypoint.testnet.solana.com:8001
  --trusted-validator 5D1fNXzvv5NjV1ysLjirC4WY92RNsVH18vjmcszZd8on
  --expected-genesis-hash 4uhcVJyU9pJkvQyS88uRDiswHXSCkY3zQawwpjk2NsNY
  --no-untrusted-rpc
  --limit-ledger-size
  ...                                # <-- your other --identity/--vote-account/etc arguments
````

     Puedes comprobar en qué ranuras tiene tu ledger: `solana-ledger-tool -l path/to/ledger lins`

3. Espere hasta que el 80% del stake esté en línea

Para confirmar que el validador reiniciado está esperando correctamente el 80%: a. Busca `N% del stake visible en gossip` mensajes de registro b. Pregunte sobre RPC en qué slot está: `solana --url http://127.0.0.1:8899 slot`. Debería devolver `SLOT_X` hasta que obtengamos 80% del stake

¡Gracias!

### Paso 7. Espera y escucha

Supervisar a los validadores cuando se reinicien. Responder preguntas, ayudar a la gente,
