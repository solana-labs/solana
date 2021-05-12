---
title: Ledger Nano S e Ledger Nano X
---

Este documento descreve como configurar um [Ledger Nano S](https://shop.ledger.com/products/ledger-nano-s) ou [Ledger Nano X](https://shop.ledger.com/pages/ledger-nano-x) com o [Ledger Live](https://www.ledger.com/ledger-live) software.

Assim que os passos de configuração mostrados abaixo estiverem concluídos e o app Solana for instalado no seu dispositivo Nano, usuários têm várias opções de como [usar o Nano para interagir com a Rede Solana](#interact-with-the-solana-network)

## Guia de Introdução

- Este documento descreve como configurar um [Ledger Nano S](https://shop.ledger.com/products/ledger-nano-s) ou [Ledger Nano X](https://shop.ledger.com/pages/ledger-nano-x) com o [Ledger Live](https://www.ledger.com/ledger-live) software.
- Siga as instruções para a configuração do dispositivo inclusa no pacote, ou [Página inicial do gerente](https://www.ledger.com/start/)
- Instale [Ledger Live desktop software](https://www.ledger.com/ledger-live/)
  - Se você já tiver o Ledger Live instalado, por favor atualize para a última versão do Ledger Live, que habilita as mais recentes atualizações de firmware e aplicativo.
- Conecte o seu Nano ao seu computador e siga as instruções na tela.
- Atualize o firmware no seu novo Nano.  Isso é necessário para garantir que você possa instalar a versão mais recente do Aplicativo Solana.
  - [Atualizar o Nano X Firmware](https://support.ledger.com/hc/en-us/articles/360002731113-Update-Ledger-Nano-S-firmware)
  - [Atualizar o Nano X Firmware](https://support.ledger.com/hc/en-us/articles/360013349800)

## Instale o App Solana no seu Nano

- Abrir Ledger ao Vivo
- Clique em "Gerente" no painel esquerdo no aplicativo e procure "Solana" no Catálogo de Aplicativos, e então clique em "Instalar".
  - Certifique-se de que seu dispositivo está conectado via USB e desbloqueado com seu PIN
- Você pode ser solicitado no Nano a confirmar a instalação do App Solana
- "Solana" agora deve ser exibido como "Instalado" no Ledger Live Manager

## Atualize para a versão mais recente do aplicativo Solana

Para certificar-se de que você tem a última funcionalidade, se você estiver usando uma versão mais antiga do Aplicativo Solana, por favor, atualize para a versão `v1.. 1` seguindo estas etapas.

- Certifique-se que você tem o Ledger Live versão 2.10.0 ou posterior.
  - Para verificar sua versão do Ledger Live, clique no botão Configurações no canto superior direito e, em seguida, clique em "Sobre".  Se uma nova versão do Ledger Live estiver disponível você deve ver um banner solicitando a atualização quando você abrir o Ledger Live.
- Atualize o firmware no seu Nano
  - [Atualizar Nano S Firmware](https://support.ledger.com/hc/en-us/articles/360002731113-Update-Ledger-Nano-S-firmware)
  - [Atualizar o Nano X Firmware](https://support.ledger.com/hc/en-us/articles/360013349800)
-  Depois de uma atualização de firmware bem-sucedida, o aplicativo Solana deve ter reinstalado automaticamente com a última versão do aplicativo.

## Interaja com a rede Solana

Os usuários podem usar qualquer uma das opções a seguir para usar seu Nano para interagir com Solana:

- [SolFlare.com](https://solflare.com/) é uma carteira web sem custódia construída especificamente para Solana e suporta transferências básicas e operações de staking com o dispositivo Ledger. Confira nosso guia para [usando um Nano com SolFlare](solflare.md).

- Desenvolvedores e usuários avançados podem [usar o Nano com as ferramentas de linha de comando Solana](hardware-wallets/ledger.md). Novos recursos de carteira são quase sempre suportados nas ferramentas de linha de comando nativas antes de serem suportados por carteiras de terceiros.

## Problemas conhecidos

- Nano X às vezes não pode conectar a carteiras da web usando o sistema Windows operacional. É provável que isto afete as carteiras baseadas no navegador que usam WebUSB. A equipe Ledger está trabalhando para resolver isso.

## Suporte

Confira nossa [página de suporte para Wallet](support.md) para ver maneiras de obter ajuda.
