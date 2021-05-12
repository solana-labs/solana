---
title: Conectandose a un cluster
---

Consulte [Solana Clusters](../clusters.md) para obtener información general sobre los clústers disponibles.

## Configurar la herramienta de línea de comandos

Puede comprobar a qué clúster se dirige actualmente la herramienta de línea de comandos (CLI) de Solana ejecutando el siguiente comando:

```bash
obtener config solana
```

Usa `solana config set` para seleccionar un clúster en particular. Después de establecer un clúster objetivo, cualquier subcomando futuro enviará/recibirá información de ese clúster.

Por ejemplo para seleccionar el clúster de Devnet, ejecutar:

```bash
configuración de solana --url https://devnet.solana.com
```

## Asegúrate de que las versiones coincidan

Aunque no es estrictamente necesario, el CLI generalmente funcionará mejor cuando su versión coincida con la versión de software ejecutándose en el clúster. Para obtener la versión CLI instalada localmente, ejecute:

```bash
solana --version
```

Para obtener la versión del cluster, ejecute:

```bash
versión de cluster de solana
```

Asegúrese de que la versión local de CLI es mayor o igual a la versión del clúster.
