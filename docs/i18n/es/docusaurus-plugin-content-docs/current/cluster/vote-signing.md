---
title: Firma de voto segura
---

Un validador recibe entradas del líder actual y envía votos que confirman que esas entradas son válidas. Este envío de votos presenta un reto de seguridad, porque los votos falsos que violan las reglas de consenso podrían ser utilizados para reducir la participación del validador.

El validador vota en el fork elegido enviando una transacción que utiliza una clave asimétrica para firmar el resultado de su trabajo de validación. Otras entidades pueden verificar esta firma usando la clave pública del validador. Si la clave del validador se utiliza para firmar datos incorrectos (por ejemplo, votos en múltiples forks del ledger), la participación del nodo o sus recursos podrían verse comprometidos.

Solana aborda este riesgo dividiendo un servicio separado _firmante de votos_ que evalúa cada voto para asegurar que no viola una condición de slashing.

## Validadores, firmantes de votos y poseedores de stake

Cuando un validador recibe múltiples bloques para la misma ranura, rastrea todos los forks posibles hasta que pueda determinar uno "mejor". Un validador selecciona el mejor fork presentando un voto a el mismo, utilizando un firmante del voto para minimizar la posibilidad de que su voto viole inadvertidamente una regla de consenso y obtenga un recorte de stake.

Un votante evalúa el voto propuesto por el validador y firma el voto sólo si no viola una condición de slashing. Un votante sólo necesita mantener el estado mínimo con respecto a los votos que firmó y los votos firmados por el resto del cluster. No necesita procesar un conjunto completo de transacciones.

Un poseedor de stake es una identidad que tiene el control del capital participado. El poseedor de stake puede delegar su participación al firmante de los votos. Una vez que se delega una stake, los votos del firmante representan el peso del voto de todo el stake delegado, y producir recompensas para todos los stake delegados.

Actualmente, hay una relación 1:1 entre los validadores y los firmantes de voto, y los poseedores de stake delegan toda su participación en un único firmante de voto.

## Servicio de firma

El servicio de firma de votos consiste en un servidor RPC JSON y un procesador de solicitudes. Al arrancar, el servicio inicia el servidor RPC en un puerto configurado y espera las solicitudes de validador. Espera el siguiente tipo de solicitudes:

1. Registrar un nuevo nodo validador

    - La solicitud debe contener la identidad del validador \(clave pública\)
    - La solicitud debe ser firmada con la clave privada del validador
    - El servicio elimina la solicitud si la firma de la solicitud no puede ser verificada
    - El servicio crea una nueva clave asimétrica de votación para el validador, y devuelve la clave pública como respuesta
    - Si un validador intenta registrarse de nuevo, el servicio devuelve la clave pública del par de claves preexistente

2. Firma un voto

    - La solicitud debe contener una transacción de votación y todos los datos de verificación
    - La solicitud debe ser firmada con la clave privada del validador
    - El servicio elimina la solicitud si la firma de la solicitud no puede ser verificada
    - El servicio verifica los datos de votación
    - El servicio devuelve una firma para la transacción

## Votación del validador

Un nodo validador, al iniciarse, crea una nueva cuenta de votos y la registra en el clúster enviando una nueva transacción de "registro de votos". Los demás nodos del clúster procesan esta transacción e incluyen el nuevo validador en el conjunto activo. Posteriormente, el validador presenta una transacción de "nuevo voto" firmada con la clave privada del validador en cada evento de votación.

### Configuración

El nodo validador está configurado con el punto final de red del servicio de firma \(IP/Port\).

### Registro

Al iniciar, el validador se registra con su servicio de firma usando JSON RPC. La llamada RPC devuelve la clave pública de votación para el nodo validador. El validador crea una nueva transacción de "registro de votos" incluyendo esta clave pública, y la envía al clúster.

### Recaudación de votos

El validador busca los votos enviados por todos los nodos del clúster en el último periodo de votación. Esta información se envía al servicio de firma con una nueva solicitud de firma de voto.

### Nueva firma de voto

El validador crea una transacción de "nuevo voto" y la envía al servicio de firma utilizando JSON RPC. La solicitud RPC también incluye los datos de verificación del voto. En caso de éxito, la llamada RPC devuelve la firma de la votación. En caso de fallo, la llamada RPC devuelve el código de falla.
