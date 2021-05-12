---
title: Verificación de Snapshot
---

## Problema

Cuando un validador arranca desde una snapshot, necesita una forma de verificar que el conjunto de cuentas coincide con lo que el resto de la red ve rápidamente. Un atacante potencial podría darle al validador un estado incorrecto, y luego tratar de convencerla de aceptar una transacción que de otro modo sería rechazada.

## Solución

Actualmente el hash bancario es derivado del estado delta de las cuentas en una ranura que luego se combina con el valor hash bancario anterior. El problema con esto es que la lista de hash crecerá en el orden del número de ranuras procesadas por la cadena y se convertirá en una carga para ambos transmitir y verificar con éxito.

Otro método ingenuo podría ser crear un árbol merkle del estado de la cuenta. Esto tiene el inconveniente de que con cada actualización de cuenta, el árbol Merkle tendría que recalcularse del estado completo de la cuenta de todas las cuentas reales del sistema.

Para verificar la instantánea, hacemos lo siguiente:

En el almacén de cuentas de lamport no nulas, hacemos un hash de los siguientes datos:

- Dueño de la cuenta
- Datos de la cuenta
- Cliente pubkey
- Balance de lamports de la cuenta
- Fork en el que se amacena la cuenta

Utilice este valor hash resultante como entrada a una función de expansión que expande el valor hash a un valor de imagen. La función creará un bloque de datos de 440 bytes donde los primeros 32 bytes son el valor de hash. y los siguientes 440 - 32 bytes son generados a partir de un RNG de Chacha con el hash como semilla.

Las imágenes de la cuenta se combinan con xor. El valor de la cuenta anterior se convertirá en el estado y el nuevo valor de la cuenta también se definirá en el estado.

Los valores hash del voto y sysvar ocurren con el hash del valor resultante de la imagen completa.

Al arrancar el validador, cuando carga desde una instantánea, verificaría el valor hash con las cuentas establecidas. Entonces usaría SPV para mostrar el porcentaje de la red que votó por el valor hash dado.

El valor resultante puede ser verificado por un validador para ser el resultado de xoring de todos los estados de cuenta corriente juntos.

Una snapshot debe purgarse de cuentas de lamport cero antes de la creación y durante la verificación ya que las cuentas de lamport cero no afectan el valor del hash, pero puede hacer que un banco validador lea que una cuenta no está presente cuando realmente debería estarlo.

Se podría hacer un ataque al estado xor para influir en su valor:

Así el tamaño de imagen de 440 bytes proviene de este papel, evitando la colisión xor con 0 \(o por lo tanto cualquier otro patrón de bit dado\): \[[https://link. pringer.com/content/pdf/10.1007%2F3-540-45708-9_19.pdf](https://link.springer.com/content/pdf/10.1007%2F3-540-45708-9_19.pdf)\]

Las matemáticas proporcionan seguridad de 128 bits en este caso:

```text
O(k * 2^(n/(1+lg(k)))
k=2^40 accounts
n=440
2^(40) * 2^(448 * 8 / 41) ~= O(2^(128))
```
