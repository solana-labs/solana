---
title: Delegar tu Stake
---

Después de que hayas [recibido SOL](transfer-tokens.md), podrías considerar usarlo delegando _stake_ a un validador. Stake es lo que llamamos tokens en una _cuenta de stake_. Solana pondera los votos de los validadores por la cantidad de stake que se les delega, lo que da a esos validadores más influencia en la determinación del siguiente bloque válido de transacciones en la cadena de bloques. Entonces Solana genera nuevos SOL periódicamente para recompensar a los jugadores y validadores. Ganas más recompensas cuanto mayor sea el stake que delegas.

## Crear una cuenta Stake

Para delegar la stake, tendrás que transferir algunos tokens a una cuenta de stake. Para crear una cuenta, necesitará un keypair. Su clave pública se utilizará como la [dirección de cuenta stake](../staking/stake-accounts.md#account-address). No hay necesidad de una contraseña o cifrado aquí; este keypair será descartado justo después de crear la cuenta stake.

```bash
solana-keygen nuevo --no-passphrase -o stake-account.json
```

La salida contendrá la clave pública después del texto `pubkey:`.

```text
pubkey: GKvqsuNcnwWqPzzuhLmGi4rzzh55FhJtGizkhHaEJqiV
```

Copie la clave pública y guárdela para su conservación. Lo necesitarás en cualquier momento que quieras realizar una acción en la cuenta stake que crees a continuación.

Ahora, crear una cuenta stake:

```bash
solana create-stake-account --from <KEYPAIR> stake-account.json <AMOUNT> \
    --stake-authority <KEYPAIR> --withdrawal -authority <KEYPAIR> \
    --fee-payer <KEYPAIR>
```

`<AMOUNT>` los tokens se transfieren desde la cuenta en el "from" `<KEYPAIR>` a una nueva cuenta stake en la clave pública de stake-account.json.

El archivo stake-account.json ahora puede ser descartado. Para autorizar acciones adicionales, utilizará el keypair `--stake-authority` o `--withdrawal -authority`, no stake-account.json.

Ver la nueva cuenta stake con el comando `solana stake-account`:

```bash
cuenta stake de solana <STAKE_ACCOUNT_ADDRESS>
```

La salida se verá similar a esto:

```text
Total Stake: 5000 SOL
La cuenta Stake: es indelegable
Stake Authority: EXU95vqs93yPeCeAU7mPPu6HbRUmTFPEiGug9oCdvQ5F
Withdraw Authority: EXU95vqs93yPeCeAU7mPPu6HbRUmTFPEiGug9oCdvQ5F
```

### Establecer Autoridades de Stake y Retiro

[Las autoridades de Stake y retiro](../staking/stake-accounts.md#understanding-account-authorities) pueden establecerse al crear una cuenta a través de las opciones `--stake-authority` y `--withdrawal -authority`, o después con el comando `solana stake-authorize`. Por ejemplo, para establecer una nueva autoridad de Stake, ejecuta:

```bash
solana stake-authorize <STAKE_ACCOUNT_ADDRESS> \
    --stake-authority <KEYPAIR> --new-stake-authority <PUBKEY> \
    --fee-payer <KEYPAIR>
```

Esto utilizará la autoridad de Staken existente `<KEYPAIR>` para autorizar una nueva autoridad de Stake `<PUBKEY>` en la cuenta Stake `<STAKE_ACCOUNT_ADDRESS>`.

### Avanzado: Dirección de cuenta de Derive Stake

Cuando delegas Stake, delegas todos los tokens en la cuenta de Stake a un validador. Para delegar a múltiples validadores, necesitarás múltiples cuentas de Stake. Crear un nuevo keypair para cada cuenta y administrar esas direcciones puede ser engorroso. Afortunadamente, puedes obtener direcciones de stake usando la opción `--seed`:

```bash
solana create-stake-account --from <KEYPAIR> <STAKE_ACCOUNT_KEYPAIR> --seed <STRING> <AMOUNT> \
    --stake-authority <PUBKEY> --withdraw-authority <PUBKEY> --fee-payer <KEYPAIR>
```

`<STRING>` es una cadena arbitraria de hasta 32 bytes, pero normalmente será un número correspondiente a la cuenta derivada. La primera cuenta puede ser "0", luego "1", y así sucesivamente. La clave pública de `<STAKE_ACCOUNT_KEYPAIR>` actúa como la dirección base. El comando deriva una nueva dirección de la dirección base y cadena de semilla. Para ver qué dirección de stake derivará el comando, usa `solana create-address-with-seed`:

```bash
solana create-address-with-seed --from <PUBKEY> <SEED_STRING> STAKE
```

`<PUBKEY>` es la clave pública de la `<STAKE_ACCOUNT_KEYPAIR>` pasada a `solana create-stake-account`.

El comando mostrará una dirección derivada, que puede utilizarse para el argumento `<STAKE_ACCOUNT_ADDRESS>` en operaciones de staking.

## Delegar tu Stake

Para delegar tu stake a un validador, necesitarás su dirección de cuenta de voto. Encuéntralo consultando al clúster para la lista de todos los validadores y sus cuentas de voto con el comando `solana validators`:

```bash
validadores de solana
```

La primera columna de cada fila contiene la identidad del validador y la segunda es la dirección de cuenta de voto. Elija un validador y utilice su dirección de cuenta de voto en `solana delegate-bet`:

```bash
solana delegate-stake --stake-authority <KEYPAIR> <STAKE_ACCOUNT_ADDRESS> <VOTE_ACCOUNT_ADDRESS> \
    --fee-payer <KEYPAIR>
```

La autoridad de stake `<KEYPAIR>` autoriza la operación en la cuenta con dirección `<STAKE_ACCOUNT_ADDRESS>`. El stake se delega en la cuenta de voto con dirección `<VOTE_ACCOUNT_ADDRESS>`.

Después de delegar stake, usa `solana stake-account` para observar los cambios a la cuenta stake:

```bash
cuenta stake de solana <STAKE_ACCOUNT_ADDRESS>
```

Verá nuevos campos "stake Delegado" y "Dirección de Cuenta de Voto Delegado" en la salida. La salida se verá similar a esto:

```text
Stake total: 5000 SOL
Créditos observados: 147462
stake Delegado: 4999. 9771712 SOL
Dirección de Cuenta de Voto Delegada: CcaHc2L43ZWjwCHART3oZoJvHLAe9hzT2DJNUpBzoTN1
El stake se activa a partir de la epoca: 42
Autoridad de stake: EXU95vqs93yPeCeAU7mPPu6HbRUmTFPEiGug9oCdvQ5F
Autoridad de Retiro: EXU95vqs93yPeCeAU7mPPu6HbRUmTFPEiGug9oCdvQ5F
```

## Desactivar Stake

Una vez delegado, puedes deseleccionar tu stake con el comando `solana deactivate-stake`:

```bash
solana deactivate-stake --stake-authority <KEYPAIR> <STAKE_ACCOUNT_ADDRESS> \
    --fee-payer <KEYPAIR>
```

La autoridad de participación `<KEYPAIR>` autoriza la operación en la cuenta con dirección `<STAKE_ACCOUNT_ADDRESS>`.

Tenga en cuenta que el stake toma varias epocas para "refrescarse". Los intentos de delegar stake en el periodo de enfriamiento fallarán.

## Retirar stake

Transferir tokens fuera de una cuenta de stake con el comando `retiro de solana`:

```bash
solana withdraw-stake --withdraw-authority <KEYPAIR> <STAKE_ACCOUNT_ADDRESS> <RECIPIENT_ADDRESS> <AMOUNT> \
    --fee-payer <KEYPAIR>
```

`<STAKE_ACCOUNT_ADDRESS>` es la cuenta de stake existente, la autoridad de participación `<KEYPAIR>` es la autoridad de retiro, y `<AMOUNT>` es el número de tokens para transferir a `<RECIPIENT_ADDRESS>`.

## Dividir stake

Puede que quieras delegar stake a validadores adicionales mientras que tu stake existente no es elegible para retirar. Puede que no sea elegible porque actualmente está en stake, enfriado o bloqueado. Para transferir tokens de una cuenta de stake existente a otra nueva, utiliza el comando `solana split-bet`:

```bash
solana split-stake --stake-authority <KEYPAIR> <STAKE_ACCOUNT_ADDRESS> <NEW_STAKE_ACCOUNT_KEYPAIR> <AMOUNT> \
    --fee-payer <KEYPAIR>
```

`<STAKE_ACCOUNT_ADDRESS>` es la cuenta de participación existente, la autoridad de stake `<KEYPAIR>` es la autoridad de stake `<NEW_STAKE_ACCOUNT_KEYPAIR>` es el keypair para la nueva cuenta, y `<AMOUNT>` es el número de tokens a transferir a la nueva cuenta.

Para dividir una cuenta stake en una dirección de cuenta derivada, utilice la opción `--seed`. Vea [Derive Stake Account Direcciones](#advanced-derive-stake-account-addresses) para más detalles.
