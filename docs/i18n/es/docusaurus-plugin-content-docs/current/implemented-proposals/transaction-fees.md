---
title: Tasas de transacción determinadas
---

En la actualidad, las transacciones incluyen un campo de comisión que indica el campo de comisión máximo que un jefe de slot puede cobrar por procesar una transacción. El cluster, por otra parte, está de acuerdo en una tasa mínima. Si la red está congestionada, el líder del slot podrá priorizar las transacciones que ofrezcan tarifas más altas. Esto significa que el cliente no sabrá cuánto se ha recogido hasta que la transacción sea confirmada por el clúster y el saldo restante sea comprobado. Esto seria exactamente como lo que no nos gusta del "gas" de Ethereum, el no determinismo.

## Tasas por congestión

Cada validador utiliza _firmas por slot_ \(SPS\) para estimar la congestión de red y _el objetivo SPS_ para estimar la capacidad de procesamiento deseada del clúster. El validador aprende el objetivo SPS de la configuración génesis, mientras que calcula el SPS de las transacciones procesadas recientemente. La configuración genesis también define un objetivo `lamports_per_signature`, que es la cuota de cargar por firma cuando el clúster está operando en el _destino SPS_.

## Calculando tarifas

El cliente utiliza la JSON RPC API para consultar el clúster para los parámetros de tarifa actuales. Estos parámetros son etiquetados con un blockhash y permanecen válidos hasta que el blockhash sea lo suficientemente viejo como para ser rechazado por el líder del slot.

Antes de enviar una transacción al clúster, un cliente puede enviar la transacción y los datos de la cuenta de comisión a un módulo SDK llamado la _fee calculator_. Mientras la versión SDK del cliente coincida con la versión de la ranura, el cliente está seguro de que su cuenta será cambiada exactamente el mismo número de lamports devueltas por la calculadora de tasas.

## Parámetros de comisiones

En la primera implementación de este diseño, el único parámetro de tarifa es `lamports_per_signature`. Cuantas más firmas tenga el cluster para verificar, mayor será la cuota. El número exacto de lamports se determina por la proporción de SPS con respecto al destino SPS. Al final de cada ranura, el clúster reduce `lamports_per_signature` cuando SPS está por debajo del objetivo y lo levanta cuando está por encima del objetivo. El valor mínimo para `lamports_per_signature` es el 50% del objetivo `lamports_per_signature` y el valor máximo es 10 x el objetivo \`lamports_per_signature'

Los parámetros futuros podrían incluir:

- `lamports_per_pubkey` - coste para cargar una cuenta
- `lamports_per_slot_distance` - mayor coste para cargar cuentas muy antiguas
- `lamports_per_byte` - costo por tamaño de la cuenta cargada
- `lamports_per_bpf_instruction` - costo para ejecutar un programa

## Ataques

### Golpeando al objetivo SPS

Un grupo de validadores puede centralizar el cluster si pueden convencerlo de que levante el objetivo SPS sobre un punto donde el resto de validadores pueden mantenerse. Aumentar el objetivo hará que caigan las tasas, generando presumiblemente más demanda y, por lo tanto, un mayor TPS. Si el validador no tiene hardware que pueda procesar tantas transacciones tan rápido, sus votos de confirmación finalmente serán tan largos que el clúster se verá obligado a arrancarlo.
