---
title: Firma de voto segura
---

## Firma de voto segura

Este diseño describe un comportamiento adicional de firma de votos que hará el proceso más seguro.

Actualmente, Solana implementa un servicio de firma de votos que evalúa cada voto para asegurar que no viola una condición de slashing. El servicio podría tener variaciones diferentes, dependiendo de las capacidades de la plataforma de hardware. En particular, podría ser utilizado junto con un enclave seguro \(como SGX\). El enclave podría generar una clave asimétrica, exponiendo una API para el usuario \(no confiable\) código para firmar las transacciones de voto, mientras se mantiene la clave privada de firma de votos en su memoria protegida.

Las siguientes secciones describen cómo funcionaría esta arquitectura:

### Flujo de mensajes

1. El nodo inicializa el enclave al iniciar

   - El enclave genera una clave asimétrica y devuelve la clave pública al

     nodo

   - El keypair es efímero. Se genera un nuevo keypair en el libro del nodo. Un

     nuevo keypair también se puede generar en tiempo de ejecución basado en algunos criterios a

     determinar.

   - El enclave devuelve su informe de certificación al nodo

2. El nodo realiza la comprobación del enclave \\(p. ej. usando las API IAS de Intel\\)

   - El nodo asegura que el Enclave Seguro se está ejecutando en un TPM y es

     firmado por un grupo de confianza

3. El poseedor de stake del nodo concede permiso a la clave efímera para utilizar su participación.

   Este proceso debe ser determinado.

4. El software no confiable del nodo llama al software de enclave confiable

   usando su interfaz para firmar transacciones y otros datos.

   - En caso de firmar votos, el nodo necesita verificar el PoH. El PoH

     es una parte integral de la firma. El enclave sería

     presentado con algunos datos verificables para comprobar antes de firmar la votación.

   - Se determinará el proceso de generación de datos verificables en un espacio no confiable

### Verificación de PoH

1. Cuando el nodo vota en una entrada `X`, hay un periodo de bloqueo `N`, para

   el cual no puede votar en un fork que no contiene `X` en su historial.

2. Cada vez que el nodo vota sobre la derivada de `X`, diga `X+y`, el

   período de bloqueo para `X` aumenta en un factor `F` \(es decir, el nodo de duración no puede votar

   un fork que no contiene aumentos de `X`\).

   - El período de bloqueo para `X+y` todavía es `N` hasta que el nodo vuelva a votar.

3. El incremento del período de bloqueo está limitado \(por ejemplo, factor `F` aplica un máximo de 32

   veces\).

4. El enclave firmante no debe firmar un voto que viole esta política. Esto

   significa

   - Enclave se inicializa con `N`, `F` y `Factor cap`
   - Enclave almacena `Factor cap` número de identificadores de entrada en los que el nodo tenía

     previamente votados

   - La solicitud de signo contiene el ID de entrada para el nuevo voto
   - Enclave verifica que el ID de entrada del nuevo voto esté en el fork correcto

     \(siguiendo las reglas \#1 y \#2 anteriores\)

### Verificación del Ancestro

Este es un enfoque alternativo, aunque menos seguro, para verificar la votación de fork. 1. El validador mantiene un conjunto activo de nodos en el cluster 2. Observa los votos del conjunto activo del último período de votación 3. Almacena el ancestro/last_tick en el que cada nodo votó 4. Envía una nueva solicitud de votación al servicio de firma de votos

- Incluye votos anteriores de los nodos en el conjunto activo, y sus

  ancestros correspondientes

  1. El firmante comprueba si los votos anteriores contienen un voto del validador,

     y el ancestro del voto coincide con la mayoría de los nodos

- Firma la nueva votación si la comprobación tiene éxito
- Se verifica \(levanta una alarma de algún orden\) si la comprobación no se ha realizado correctamente

La premisa es que el validador puede ser suplantado como máximo una vez para votar datos incorrectos. Si alguien secuestra al validador y presenta una solicitud de voto para datos falsos ese voto no será incluido en el PoH \(como será rechazado por el cluster\). La próxima vez que el validador envíe una solicitud para firmar el voto, el servicio de firma detectará que falta el último voto del validador \(como parte de

## 5 anteriores\).

### Determinación del fork

Debido al hecho de que el enclave no puede procesar PoH, no tiene conocimiento directo del historial del fork de un voto de validador presentado. Cada enclave debe iniciarse con el actual _conjunto activo_ de claves públicas. Un validador debe presentar su voto actual junto con los votos del conjunto activo \(incluyendo sí mismo\) que observó en la ranura de su voto anterior. De esta forma, el enclave puede suponer los votos que acompañan a la votación anterior del validador y, por tanto, al fork sobre el que se vota. Esto no es posible para la votación inicial presentada por el validador, ya que no tendrá una franja horaria "anterior" como referencia. Para tener en cuenta esto, se debe aplicar un corto bloqueo de votación hasta que se presente el segundo voto que contenga los votos dentro del conjunto activo, junto con su propio voto, en el momento álgido de la votación inicial.

### Configuración de Enclave

Un cliente que hace staking debe ser configurable para evitar votar en forks inactivos. Este mecanismo debe usar el conjunto activo conocido del cliente `N_active` junto con un umbral de votación `N_vote` y una profundidad de umbral `N_depth` para determinar si continuar o no votando sobre un fork enviado. Esta configuración debería tomar la forma de una regla de tal manera que el cliente solo votará en un fork si observa más de `N_vote` en `N_depth`. Prácticamente, esto representa que el cliente confirme que ha observado cierta probabilidad de finalidad económica del fork presentado a una profundidad en la que un voto adicional crearía un bloqueo durante un tiempo indeseable si ese fork resulta no estar vivo.

### Desafíos

1. Generación de datos verificables en espacio no fiable para la verificación de PoH en el

   enclave.

2. Se necesita una infraestructura para conceder participación a una clave efímera.
