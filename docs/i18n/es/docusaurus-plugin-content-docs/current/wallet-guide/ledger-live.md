---
title: Ledger Nano S y Ledger Nano X
---

Este documento describe cómo configurar un [Ledger Nano S](https://shop.ledger.com/products/ledger-nano-s) o [Ledger Nano X](https://shop.ledger.com/pages/ledger-nano-x) con el software [Ledger Live](https://www.ledger.com/ledger-live).

Una vez que los pasos de configuración mostrados a continuación están completados y la aplicación Solana está instalada en tu dispositivo Nano, los usuarios tienen varias opciones de cómo [usar el Nano para interactuar con la Red Solana](#interact-with-the-solana-network)

## Comenzando

- Ordena un [Nano S](https://shop.ledger.com/products/ledger-nano-s) o [Nano X](https://shop.ledger.com/pages/ledger-nano-x) de Ledger.
- Siga las instrucciones para la configuración del dispositivo incluida en el paquete, o [página de inicio del Ledger's](https://www.ledger.com/start/)
- Instalar [software de escritorio Ledger Live](https://www.ledger.com/ledger-live/)
  - Si ya tiene instalado Ledger Live, actualice a la última versión de Ledger Live, que habilita las actualizaciones más recientes de firmware y aplicaciones.
- Conecta tu Nano a tu ordenador y sigue las instrucciones en pantalla.
- Actualice el firmware en su nuevo Nano. Esto es necesario para asegurarte de que puedes instalar la última versión de la aplicación Solana.
  - [Actualizar Firmware de Nano S](https://support.ledger.com/hc/en-us/articles/360002731113-Update-Ledger-Nano-S-firmware)
  - [Actualizar Firmware Nano X](https://support.ledger.com/hc/en-us/articles/360013349800)

## Instala la aplicación Solana en tu Nano

- Abrir Ledger Live
- Haga clic en "Administrador" en el panel izquierdo de la aplicación y busque "Solana" en el Catálogo de aplicaciones, luego haga clic en "Instalar".
  - Asegúrate de que tu dispositivo está conectado por USB y está desbloqueado con su PIN
- Se te pedirá en el Nano que confirmes la instalación de Solana App
- "Solana" ahora debe mostrarse como "Instalado" en el Ledger Live Manager

## Actualizar a la última versión de la aplicación Solana

Para asegurarse de que tiene la última funcionalidad, si está utilizando una versión anterior de la aplicación Solana, por favor actualice a la versión `v1.. 1` siguiendo estos pasos.

- Asegúrese de que tiene Ledger Live versión 2.10.0 o posterior.
  - Para comprobar su versión de Ledger Live, haga clic en el botón Ajustes en la esquina superior derecha, luego haga clic en "Acerca de". Si una nueva versión de Ledger Live está disponible, deberías ver un banner que te pide que actualices cuando abras Ledger Live.
- Actualice el firmware en su Nano
  - [Actualizar Firmware de Nano S](https://support.ledger.com/hc/en-us/articles/360002731113-Update-Ledger-Nano-S-firmware)
  - [Actualizar Firmware Nano X](https://support.ledger.com/hc/en-us/articles/360013349800)
- Después de una actualización exitosa del firmware, la aplicación Solana debería reinstalarse automáticamente con la última versión de la aplicación.

## Interactuar con la red Solana

Los usuarios pueden usar cualquiera de las siguientes opciones para usar su Nano para interactuar con Solana:

- [SolFlare.com](https://solflare.com/) es un monedero web no custodial construido específicamente para Solana y soporta transferencias básicas y operaciones de staking con el dispositivo Ledger. Echa un vistazo a nuestra guía para [usando un Nano con SolFlare](solflare.md).

- Los desarrolladores y usuarios avanzados pueden [usar un Nano con las herramientas de línea de comandos de Solana](hardware-wallets/ledger.md). Las nuevas características del monedero casi siempre son soportadas en las herramientas nativas de línea de comandos antes de ser soportadas por carteras de terceros.

## Problemas conocidos

- A veces Nano X no puede conectarse a carteras web usando el sistema operativo Windows. Es probable que esto afecte a cualquier monedero basado en el navegador que utilice WebUSB. El equipo de Ledger está trabajando para resolver esto.

## Soporte

Echa un vistazo a nuestra [Página de soporte al monedero](support.md) para encontrar formas de obtener ayuda.
