---
title: Iniciando un Validador
---

## Configurar Solana CLI

La solana cli incluye los comandos de configuración `get` y `set` para establecer automáticamente el argumento `--url` de los comandos cli. Por ejemplo:

```bash
configuración de solana --url http://devnet.solana.com
```

Mientras que esta sección muestra cómo conectarse al clúster de Devnet, los pasos son similares para los otros [clústers Solana](../clusters.md).

## Confirmar que el Cluster es alcanzable

Antes de adjuntar un nodo validador, compruebe que el clúster sea accesible a su máquina obteniendo el contador de transacciones:

```bash
solana transaction-count
```

Ver el panel de [métricas](https://metrics.solana.com:3000/d/monitor/cluster-telemetry) para más detalles sobre la actividad del clúster.

## Confirme su instalación

Intenta ejecutar el siguiente comando para unirte a la red gossip y ver todos los otros nodos en el clúster:

```bash
solana-gossip spy --entrypoint devnet.solana.com:8001
# Press ^C to exit
```

## Habilitando CUDA

Si su máquina tiene un GPU con CUDA instalado \(Linux-only actualmente\), incluya el argumento `--cuda` a `solana-validator`.

Cuando se inicie su validador, busque el siguiente mensaje de registro para indicar que CUDA está habilitado: `"[<timestamp> solana::validator] CUDA is enabled"`

## Ajuste del sistema

### Linux

#### Automático

El repo de solana incluye un daemon para ajustar la configuración del sistema para optimizar el rendimiento (es decir, aumentando el buffer UDP del OS y los límites de mapeo de archivos).

El daemon (`solana-sys-tuner`) está incluido en la versión binaria de solana. Reinicie, *antes de*reiniciar su validador, después de cada actualización de software para asegurarse de que se apliquen los últimos ajustes recomendados.

Para ejecutarlo:

```bash
sudo solana-sys-tuner --user $(whoami) > sys-tuner.log 2>&1 &
```

#### Manual

Si prefiere administrar la configuración del sistema por sí solo, puede hacerlo con los siguientes comandos.

##### **Aumentar búferes UDP**

```bash
sudo bash -c "cat >/etc/sysctl.d/20-solana-udp-buffers.conf <<EOF
# Increase UDP buffer size
net.core.rmem_default = 134217728
net.core.rmem_max = 134217728
net.core.wmem_default = 134217728
net.core.wmem_max = 134217728
EOF"
```

```bash
sudo sysctl -p iger/sysctl.d/20-solana-udp-buffers.conf
```

##### **Aumento del límite de archivos asignados a memoria**

```bash
sudo bash -c "cat >/etc/sysctl.d/20-solana-mmaps.conf <<EOF
# Increase memory mapped files limit
vm.max_map_count = 500000
EOF"
```

```bash
sudo sysctl -p /etc/sysctl.d/20-solana-mmaps.conf
```

Añadir

```
LimitNOFILE=500000
```

a la sección `[Service]` de su archivo de servicio del sistema, si utiliza uno, de lo contrario añada

```
DefaultLimitNOFILE=500000
```

a la sección `[Manager]` de `/etc/systemd/system.conf`.

```bash
sudo systemctl daemon-reload
```

```bash
sudo bash -c "cat >/etc/security/limits.d/90-solana-nofiles.conf <<EOF
# Increase process file descriptor count limit
* - nofile 500000
EOF"
```

```bash
### Close all open sessions (log out then, in again) ###
```

## Generar identidad

Crear un keypair de identidad para su validador ejecutando:

```bash
keygen nuevo -o ~/validator-keypair.json
```

La clave pública de identidad ahora se puede ver ejecutando:

```bash
pubkey de solana-keygen ~/validator-keypair.json
```

> Nota: El archivo "validator-keypair.json" también es su clave privada \(ed25519\).

### Identidad de Billetera de papel

Puede crear una cartera de papel para su archivo de identidad en lugar de escribir el archivo keypair en el disco con:

```bash
solana-keygen nuevo --no-outfile
```

La correspondiente clave pública de identidad se puede ver ejecutando:

```bash
pubkey de solana-keygen
```

y luego introduzca su frase de semilla.

Ver [Uso de la billetera de papel](../wallet-guide/paper-wallet.md) para más información.

---

### Vanity Keypair

Puede generar un vanity keypair personalizado usando solana-keygen. Por ejemplo:

```bash
moler solana-keygen --starts-with e1v1s:1
```

Dependiendo de la cadena solicitada, puede tardar días en encontrar una coincidencia...

---

El keypair de identificación del validador identifica a su validador dentro de la red. **Es crucial respaldar esta información.**

Si no hace una copia de seguridad de esta información, NO PODRÁ RECUPERAR SU VALIDADOR, si pierde el acceso a él. Si esto ocurre, USTED PERDERE SU ASIGNACIÓN DE SOL.

Para hacer una copia de seguridad de su keypair de identificación del validador, **haga una copia de seguridad de su "validator-keypair.json" o su frase semilla en una ubicación segura.**

## Más configuración de Solana CLI

Ahora que tiene un keypair, configure la configuración solana para usar elkeypair del validador para todos los siguientes comandos:

```bash
conjunto de configuración de solana --keypair ~/validator-keypair.json
```

Debería ver la siguiente salida:

```text
Wallet Config Updated: /home/solana/.config/solana/wallet/config.yml
* url: http://devnet.solana.com
* keypair: /home/solana/validator-keypair.json
```

## Airdrop & comprobar el saldo del validador

Airdrop algunos SOL para comenzar:

```bash
solana airdrop 10
```

Tenga en cuenta que los airdrops sólo están disponibles en Devnet y Testnet. Ambos están limitados a 10 SOL por solicitud.

Para ver su saldo actual:

```text
saldo de solana
```

O para ver con más detalle:

```text
saldo de solana --lamports
```

Lea más sobre la diferencia [entre SOL y lamports aquí](../introduction.md#what-are-sols).

## Crear cuenta de voto

Si aún no lo ha hecho, cree un keypair de cuenta de voto y cree la cuenta de voto en la red. Si has completado este paso, deberías ver el "vote-account-keypair.json" en el directorio de tiempo de ejecución de Solana:

```bash
keygen nuevo -o ~/vote-account-keypair.json
```

El siguiente comando puede utilizarse para crear tu cuenta de voto en el blockchain con todas las opciones predeterminadas:

```bash
solana create-vote-account ~/vote-account-keypair.json ~/validator-keypair.json
```

Lea más sobre [crear y administrar una cuenta de voto](vote-accounts.md).

## Validadores de confianza

Si conoce y confía en otros nodos validadores, puede especificar esto en la línea de comandos con el argumento `--trusted-validator <PUBKEY>` a `solana-validator`. Puede especificar múltiples si se repite el argumento `--trusted-validator <PUBKEY1> --trusted-validator <PUBKEY2>`. Esto tiene dos efectos, uno es cuando el validador arranca con `--no-untrusted-rpc`, sólo pedirá ese conjunto de nodos de confianza para descargar datos génesis y snapshot. Otro es que en combinación con la opción `--halt-on-trusted-validator-hash-mismatch`, supervisará el hash root de merkle del estado completo de las cuentas de otros nodos de confianza en gossip y si los hashes producen algún desajuste, el validador detendrá el nodo para evitar que el validador vote o procese valores de estado potencialmente incorrectos. Por el momento, la ranura en la que el validador publica el hash está atado al intervalo snapshot. Para que la característica sea efectiva, todos los validadores en el conjunto de confianza deben establecerse en el mismo valor del intervalo snapshot o múltiplos del mismo.

Es altamente recomendable que utilice estas opciones para evitar la descarga de estado snapshot maliciosa o la divergencia de estado de la cuenta.

## Conecte su validador

Conectar al clúster ejecutando:

```bash
solana-validator \
  --identity ~/validator-keypair.json \
  --vote-account ~/vote-account-keypair.json \
  --ledger ~/validator-ledger \
  --rpc-port 8899 \
  --entrypoint devnet.solana.com:8001 \
  --limit-ledger-size \
  --log ~/solana-validator.log
```

Para forzar el registro del validador en la consola agregue un argumento `--log -`, de lo contrario el validador se registrará automáticamente en un archivo.

> Nota: Puede utilizar una [frase de semilla de la billetera de papel](../wallet-guide/paper-wallet.md) para su `--identity` y/o `--authorized-voter` keypairs. Para usarlos, pasar el argumento respectivo como `solana-validator --identity ASK ... --authorized-voter ASK ...` y se le pedirá que introduzca sus frases de semilla y contraseña opcional.

Confirme que su validador se conectó a la red abriendo un nuevo terminal y ejecutando:

```bash
solana-gossip spy --entrypoint devnet.solana.com:8001
```

Si su validador está conectado, su clave pública y su dirección IP aparecerán en la lista.

### Controlando la asignación de puertos de red locales

Por defecto, el validador seleccionará dinámicamente los puertos de red disponibles en el rango 8000-10000 y puede ser reemplazado con `--dynamic-port-range`. Por ejemplo, `solana-validator --dynamic-port-range 11000-11010 ...` restringirá el validador a los puertos 11000-11010.

### Limitando el tamaño del ledger para conservar espacio en disco

El parámetro `--limit-ledger-size` le permite especificar cuántos ledger [shreds](../terminology.md#shred) su nodo retiene en el disco. Si no incluye este parámetro, el validador mantendrá el libro de valores entero hasta que se agote de espacio en disco.

El valor predeterminado intenta mantener el uso del disco ledger por debajo de 500GB. Se puede solicitar más o menos uso del disco añadiendo un argumento a `--limit-ledger-size` si se desea. Compruebe `solana-validator --help` para conocer el valor límite por defecto utilizado por `--limit-ledger-size`. Más información sobre seleccionar un valor límite personalizado está [disponible aquí](https://github.com/solana-labs/solana/blob/583cec922b6107e0f85c7e14cb5e642bc7dfb340/core/src/ledger_cleanup_service.rs#L15-L26).

### Unidad de sistema

Ejecutar el validador como una unidad systemd es una forma sencilla de gestionar la ejecución en segundo plano.

Asumiendo que tiene un usuario llamado `sol` en su máquina, cree el archivo `/etc/systemd/system/sol.service` con lo siguiente:

```
[Unit]
Description=Solana Validator
After=network.target
Wants=solana-sys-tuner.service
StartLimitIntervalSec=0

[Service]
Type=simple
Restart=always
RestartSec=1
User=sol
LimitNOFILE=500000
LogRateLimitIntervalSec=0
Environment="PATH=/bin:/usr/bin:/home/sol/.local/share/solana/install/active_release/bin"
ExecStart=/home/sol/bin/validator.sh

[Install]
WantedBy=multi-user.target
```

Ahora crea `/home/sol/bin/validator.sh` para incluir la línea de comandos deseada `solana-validator`. Asegúrese de que la ejecución de `/home/sol/bin/validator.sh` inicia manualmente el validador como se esperaba. No te olvides de marcarlo como ejecutable con `chmod +x /home/sol/bin/validator.sh`

Iniciar el servicio con:

```bash
$ sudo systemctl habilitar --now sol
```

### Registrandose

#### Ajuste de la salida del registro

Los mensajes que un validador emite al registro pueden ser controlados por la variable de entorno `RUST_LOG`. Los detalles pueden encontrarse en la [documentación](https://docs.rs/env_logger/latest/env_logger/#enabling-logging) para la `caja env_logger` Caja.

Tenga en cuenta que si la salida de registro se reduce, esto puede hacer difícil depurar problemas encontrados más tarde. En caso de que se solicite ayuda al equipo, será necesario revertir los cambios y reproducir el problema antes de que se pueda proporcionar ayuda.

#### Registro de rotación

El archivo de registro del validador, tal como fue especificado por `--log ~/solana-validator. og`, puede obtener muy grande con el tiempo y se recomienda configurar la rotación del registro.

El validador volverá a abrirlo cuando reciba la señal `USR1`, que es el primitivo básico que permite la rotación de registros.

#### Usando logrotate

Un ejemplo de configuración para el `logrotate`, el cual asume que el validador está corriendo como un servicio de sistema llamado `sol. service` y escribe un archivo de registro en /home/sol/solana-validator.log:

```bash
# Setup log rotation

cat > logrotate.sol <<EOF
/home/sol/solana-validator.log {
  rotate 7
  daily
  missingok
  postrotate
    systemctl kill -s USR1 sol.service
  endscript
}
EOF
sudo cp logrotate.sol /etc/logrotate.d/sol
systemctl restart logrotate.service
```

### Desactivar comprobaciones de puertos para acelerar reinicios

Una vez que el validador está funcionando normalmente, puede reducir el tiempo que toma reiniciar su validador añadiendo la bandera `--no-port-check` a su línea de comandos `solana-validator`.

### Desactivar la compresión de Snapshot para reducir el uso de CPU

Si no estás sirviendosnapshots a otros validadores, la compresión de snapshot puede desactivarse para reducir la carga de la CPU a expensas de un poco más de uso de disco para almacenamiento local de snapshots.

Agregue el argumento `--snapshot-compression none` a los argumentos de la línea de comandos `solana-validator` y reinicie el validador.

### Utilizando un ramdisk con spill-over en swap para la base de datos de cuentas para reducir el desgaste SSD

Si su máquina tiene un montón de RAM, un ramdisk ([tmpfs](https://man7.org/linux/man-pages/man5/tmpfs.5.html)) puede ser usado para mantener la base de datos de cuentas

Al usar tmpfs también es esencial configurar el intercambio en su máquina, así como evitar que se agote el espacio tmpfs periódicamente.

Se recomienda una partición tmpfs de 300GB, con una partición de intercambio de 250GB.

Ejemplo de configuración:

1. `sudo mkdir /mnt/solana-accounts`
2. Añade una parición tmpfs de 300GB añadiendo una nueva línea que contenga `tmpfs/mnt/solana-accounts tmpfs rw,size=300G,user=sol 0 0` a `/etc/fstab` (asumiendo que su validador se está ejecutando bajo el usuario "sol"). **CUIDADO: Si edita incorrectamente /etc/fstab su máquina puede dejar de arrancar**
3. Crear al menos 250GB de espacio de intercambio

- Elija un dispositivo para usar en lugar de `SWAPDEV` para el resto de estas instrucciones. Seleccione Idealmente una partición de disco libre de 250GB o superior en un disco rápido. Si uno no está disponible, cree un archivo de intercambio con `sudo dd if=/dev/zero of=/swapfile bs=1MiB count=250KiB`, establece sus permisos con `sudo chmod 0600 /swapfile` y usa `/swapfile` como `SWAPDEV` para el resto de estas instrucciones
- Formatear el dispositivo para su uso como intercambio con `sudo mkswap SWAPDEV`

4. Añade el archivo de intercambio a `/etc/fstab` con una nueva línea que contenga `SWAPDEV swap swap defaults 0 0`
5. Habilitar intercambio con `sudo swapon -a` y montar tmpfs con `sudo mount /mnt/solana-accounts/`
6. Confirmar que el swap está activo con `free -g` y que el tmpfs está montado con `mount`

Ahora añada el argumento `--accounts /mnt/solana-accounts` a los argumentos de línea de comandos `solana-validator` y reinicia el validador.

### Indexación de cuenta

A medida que el número de cuentas pobladas en el clúster crece, las solicitudes RPC de datos de cuentas que exploran todo el conjunto de cuentas - como[`getProgramAccounts`](developing/clients/jsonrpc-api.md#getprogramaccounts) y [solicitudes específicas de SPL-token](developing/clients/jsonrpc-api.md#gettokenaccountsbydelegate) - pueden tener un rendimiento deficiente. Si su validador necesita soporte para cualquiera de estas peticiones, puede usar el parámetro `--account-index` para activar uno o más índices de cuenta en memoria que mejoran significativamente el rendimiento de RPC indexando cuentas por el campo clave. Actualmente soporta los siguientes valores de parámetro:

- `program-id`: cada cuenta indexada por su programa de propiedad; utilizada por [`getProgramAccounts`](developing/clients/jsonrpc-api.md#getprogramaccounts)
- `spl-token-mint`: cada cuenta SPL token indexada por su token Mint; utilizada por [getTokenAccountsByDelegate](developing/clients/jsonrpc-api.md#gettokenaccountsbydelegate)y [getTokenLargestAccounts](developing/clients/jsonrpc-api.md#gettokenlargestaccounts)
- `spl-token-owner`: cada cuenta SPL token indexada por la dirección del dueño del token; utilizado por [getTokenAccountsByOwner](developing/clients/jsonrpc-api.md#gettokenaccountsbyowner)y [`getProgramAccounts`](developing/clients/jsonrpc-api.md#getprogramaccounts) solicitudes que incluyen un filtro spl-token-owner.
