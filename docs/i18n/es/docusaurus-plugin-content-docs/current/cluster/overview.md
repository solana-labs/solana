---
title: Un clúster de Solana
---

Un clúster de Solana es un conjunto de validadores que trabajan juntos para atender las transacciones de los clientes y mantener la integridad del libro mayor. Muchos clusters pueden coexistir. Cuando dos clusters comparten un bloque de génesis común, intentan converger. De lo contrario, simplemente ignoran la existencia de la otra. Las transacciones enviadas a la persona equivocada se rechazan silenciosamente. En esta sección, discutiremos cómo se crea un clúster, cómo los nodos se unen al clúster, cómo comparten el libro mayor, cómo se aseguran de que el libro mayor se replique y cómo lidian con los nodos con errores y maliciosos.

## Creando un Cluster

Antes de iniciar cualquier validación, primero es necesario crear una _ configuración de genesis _. La configuración hace referencia a dos claves públicas, una _ mint _ y un _ validador de arranque _. El validador que tiene la clave privada del validador de arranque es responsable de agregar las primeras entradas al libro mayor. Inicializa su estado interno con la cuenta del mint. Esta cuenta mantendrá el número de tokens nativos definidos por la configuración del génesis. El segundo validador se pone en contacto con el validador de arranque para registrarse como _ validador _. Luego, los validadores adicionales se registran con cualquier miembro registrado del clúster.

Un validador recibe todas las entradas del líder y envía votos confirmando que esas entradas son válidas. Después de votar, se espera que el validador guarde esas entradas. Una vez que el validador observa un número suficiente de copias existen, elimina su copia.

## Uniéndose a un Cluster

Los validadores ingresan al clúster a través de mensajes de registro enviados a su _ plano de control _. El plano de control se implementa mediante un protocolo _ gossip _, lo que significa que un nodo puede registrarse con cualquier nodo existente y esperar que su registro se propague a todos los nodos del clúster. El tiempo que tardan todos los nodos en sincronizarse es proporcional al cuadrado del número de nodos que participan en el clúster. Algorítmicamente, eso se considera muy lento, pero a cambio de ese tiempo, se le asegura a un nodo que eventualmente tendrá la misma información que cualquier otro nodo, y que esa información no puede ser censurada por ningún nodo.

## Enviando transacciones a un cluster

Los clientes envían transacciones al puerto de la Unidad de procesamiento de transacciones \ (TPU \) de cualquier validador. Si el nodo está en el rol de validador, reenvía la transacción al líder designado. Si en la función de líder, el nodo agrupa las transacciones entrantes, las marca de tiempo para crear una _ entrada _ y las coloca en el plano de datos _ del clúster. _. Una vez en el plano de datos, las transacciones son validadas por los nodos de validación, agregándolas efectivamente al libro mayor.

## Confirmando transacciones

Un clúster de Solana es capaz de realizar una _ confirmación _ de hasta 150 nodos con planes de escalar hasta cientos de miles de nodos. Una vez implementado por completo, se espera que los tiempos de confirmación aumenten solo con el logaritmo del número de validadores, donde la base del logaritmo es muy alta. Si la base es mil, por ejemplo, significa que para los primeros mil nodos, la confirmación será la duración de tres saltos de red más el tiempo que tarda el validador más lento de una supermayoría en votar. Para los siguientes millones de nodos, la confirmación aumenta en un solo salto de red.

Solana define la confirmación como la duración del tiempo desde que el líder marca el tiempo de una nueva entrada al momento en que reconoce una supermayoría de los votos del contador.

Una red de chismes es demasiado lenta para lograr una confirmación en un segundo una vez que la red crece más allá de cierto tamaño. El tiempo que tarda en enviar mensajes a todos los nodos es proporcional al cuadrado del número de nodos. Si un blockchain quiere conseguir una baja confirmación e intenta hacerlo usando una red de chismes, se verá obligado a centralizar a un puñado de nodos.

La confirmación escalable se puede conseguir utilizando la siguiente combinación de técnicas:

1. Marque las transacciones con una muestra de VDF y firme la marca de tiempo.
2. Divida las transacciones en lotes, envíe cada una a nodos separados y haga

   que cada nodo comparta su lote con sus pares.

3. Repita el paso anterior de forma recursiva hasta que todos los nodos tengan todos los lotes.

Solana gira a los líderes en intervalos fijos, llamados _espacios_. Cada líder sólo puede producir entradas durante su ranura asignada. Por lo tanto, el líder marca el tiempo de las transacciones para que los validadores puedan buscar la clave pública del líder designado. El líder firma entonces la marca de tiempo para que un validador pueda verificar la firma, demostrando que el firmante es el propietario de la clave pública del líder designado.

A continuación, las transacciones se dividen en lotes para que un nodo pueda enviar transacciones a múltiples partes sin hacer copias múltiples. Si, por ejemplo, el líder necesitaba enviar 60 transacciones a 6 nodos, rompería esa colección de 60 en lotes de 10 transacciones y enviaría uno a cada nodo. Esto le permite al líder colocar 60 transacciones en el cable, no 60 transacciones para cada nodo. Cada nodo comparte su lote con sus pares. Una vez que el nodo ha recogido los 6 lotes, reconstruye el conjunto original de 60 transacciones.

Un lote de transacciones sólo se puede dividir tantas veces antes de que sea tan pequeño que la información de cabecera se convierte en el principal consumidor del ancho de banda de la red. En el momento de esta escritura, el enfoque está escalando bien hasta unos 150 validadores. Para escalar hasta cientos de miles de validadores, cada nodo puede aplicar la misma técnica que el nodo de líder a otro conjunto de nodos de igual tamaño. A la técnica la llamamos [ _ Turbine Block Propogation _ ](turbine-block-propagation.md).
