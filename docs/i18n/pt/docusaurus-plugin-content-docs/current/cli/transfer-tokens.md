---
title: Enviar e Receber Tokens
---

Esta página descreve como receber e enviar tokens SOL usando as ferramentas de linha de comando com uma carteira de linha de comando como um papel de [wallet](../wallet-guide/paper-wallet.md), uma carteira de sistema de arquivos [](../wallet-guide/file-system-wallet.md), ou uma carteira de hardware [](../wallet-guide/hardware-wallets.md). Antes de começar, certifique-se que você criou uma carteira e tem acesso ao seu endereço (pubkey) e ao conjunto de chaves de login. Confira nossas [convenções para inserir pares de chaves para diferentes tipos de carteira](../cli/conventions.md#keypair-conventions).

## Testando sua Carteira

Antes de compartilhar sua chave pública com os outros, você pode primeiro garantir que a chave é válida e que você de fato mantém a chave privada correspondente.

Neste exemplo, criaremos uma segunda carteira além da sua primeira carteira, e, em seguida, transferiremos alguns tokens para ela. Isto confirmará que você pode enviar e receber tokens no tipo de carteira que você escolhe.

Este exemplo de teste usa nosso Testnet de Desenvolvedor, chamado devnet. Tokens emitidos no devnet não possuem **nenhum valor**, então não se preocupe se você perdê-los.

#### Airdrop de alguns tokens para começar

Primeiro, _airdrop_ você mesmo joga tokens no devnet.

```bash
solana airdrop 1 <RECIPIENT_ACCOUNT_ADDRESS> --url https://devnet.solana.com
```

onde você substitui o texto `<RECIPIENT_ACCOUNT_ADDRESS>` com seu codificado em base58 chave pública/ endereço carteira.

#### Verifique seu saldo

Confirme se o Airdrop foi bem sucedido e verifique o saldo da conta. Ele deve retornar `1 SOL`:

```bash
solana airdrop <ACCOUNT_ADDRESS> --url https://devnet.solana.com
```

#### Criae um segundo endereço da carteira

Precisamos de um novo endereço para receber tokens. Crie um segundo par de chaves e registre seu pubkey:

```bash
solana-keygen new --no-passphrase --no-outfile
```

A saída ira ter o endereço após o texto `pubkey:`. Copie o endereço. Nós o usaremos na próxima etapa.

```text
pubkey: GKvqsuNcnwWqPzzuhLmGi4rzzh55FhJtGizkhHaEJqiV
```

Você também pode criar uma segunda (ou mais) carteira de qualquer tipo: [paper](../wallet-guide/paper-wallet#creating-multiple-paper-wallet-addresses), [file system](../wallet-guide/file-system-wallet.md#creating-multiple-file-system-wallet-addresses), or [hardware](../wallet-guide/hardware-wallets.md#multiple-addresses-on-a-single-hardware-wallet).

#### Transferir tokens de sua primeira carteira para o segundo endereço

Em seguida, prove que você tem os tokens do airdrop, transferindo. O cluster Solana só aceitará a transferência se você assinar a transação com o par de chaves privadas correspondendo à chave pública do remetente no transação.

```bash
solana transfer --from <KEYPAIR> <RECIPIENT_ACCOUNT_ADDRESS> 0.5 --allow-unfunded-recipient --url https://devnet.solana.com --fee-payer <KEYPAIR>
```

onde você substitui `<KEYPAIR>` pelo caminho para um par de chaves em sua primeira carteira, e substitua `<RECIPIENT_ACCOUNT_ADDRESS>` pelo endereço de sua segunda carteira.

Confirme os saldos atualizados com `solana balance`:

```bash
solana airdrop <ACCOUNT_ADDRESS> --url http://devnet.solana.com
```

onde `<ACCOUNT_ADDRESS>` é a chave pública do seu par de chaves ou a chave pública do beneficiário.

#### Exemplo completo da transferência de testes

```bash
$ solana-keygen new --outfile meu_solana_wallet. son # Criando minha primeira carteira, uma carteira de sistema de arquivos
Gerando um novo par de chaves
Para aumentar a segurança digite uma senha (vazia para nenhuma frase-passe):
Wrote novo par de chaves para minha_solana_wallet. son
==================================================================
pubkey: DYw8jCTfwHNRJhhmFcbXvVDTqWMEVFBX6ZKUmG5CNSKK # Aqui está o endereço da primeira carteira
==========================================================================================
Salve esta semente para recuperar seu novo keypair:
largura no concerto com chover eterno de tag spy guard # Se isso for uma verdadeira valente, nunca compartilhe estas palavras na internet como essa!
==========================================================================

$ solana airdrop 1 DYw8jCTfwHNRJhhmFcbXvVDTqWMEVFBX6ZKUmG5CNSKK --url https://devnet.solana.com  # Airdropping 1 SOL to my wallet's address/pubkey
Requesting airdrop of 1 SOL from 35.233.193.70:9900
1 SOL

$ solana balance DYw8jCTfwHNRJhhmFcbXvVDTqWMEVFBX6ZKUmG5CNSKK --url https://devnet.solana.com # Check the address's balance
1 SOL

$ solana-keygen new --no-outfile  # Creating a second wallet, a paper wallet
Generating a new keypair
For added security, enter a passphrase (empty for no passphrase):
====================================================================
pubkey: 7S3P4HxJpyyigGzodYwHtCxZyUQe9JiBMHyRWXArAaKv                   # Here is the address of the second, paper, wallet.
====================================================================
Salve esta frase de semente para recuperar o seu novo par chave:
primo de pânico machucado sobre a carga da costa engaja-se e ganhe o amor # Se esta for uma carteira real, nunca compartilhe essas palavras na internet como essa!
====================================================================

$ solana transfer --from my_solana_wallet.json 7S3P4HxJpyyigGzodYwHtCxZyUQe9JiBMHyRWXArAaKv 0.5 --allow-unfunded-recipient --url https://devnet.solana.com --fee-payer my_solana_wallet.json  # Transferring tokens to the public address of the paper wallet
3gmXvykAd1nCQQ7MjosaHLf69Xyaqyq1qw2eu1mgPyYXd5G4v1rihhg1CiRw35b9fHzcftGKKEu4mbUeXY2pEX2z  # This is the transaction signature

$ solana balance DYw8jCTfwHNRJhhmFcbXvVDTqWMEVFBX6ZKUmG5CNSKK --url https://devnet.solana.com
0.499995 SOL  # The sending account has slightly less than 0.5 SOL remaining due to the 0.000005 SOL transaction fee payment

$ solana balance 7S3P4HxJpyyigGzodYwHtCxZyUQe9JiBMHyRWXArAaKv --url https://devnet.solana.com
0.5 SOL  # The second wallet has now received the 0.5 SOL transfer from the first wallet

```

## Receber Tokens

Para receber tokens, você precisará de um endereço para que outros enviem tokens. No Solana, o endereço da carteira é a chave pública de um conjunto de chaves. Existe uma variedade de técnicas para gerar pares chave. O método que você escolher dependerá de como você optar por armazenar chaves. Os pares de chave são armazenados nas carteiras. Antes de receber tokens, você precisa [criar uma carteira](../wallet-guide/cli.md). Uma vez concluído, você deve ter uma chave pública para cada par de chaves que você gerou. A chave pública é uma longa sequência de caracteres base58. Seu comprimento varia de 32 a 44 caracteres.

## Envie Tokens

Se você já possui SOL e quer enviar tokens para alguém, você vai precisar de um caminho para seu par de teclado, suas chaves públicas codificadas na base58, e um número de tokens para transferir. Assim que você tiver isso coletado, você pode transferir tokens com o comando `solana transferência`:

```bash
transferência de solana --from <KEYPAIR> <RECIPIENT_ACCOUNT_ADDRESS> --url --fee-payer <AMOUNT>
```

Confirme os saldos atualizados com `solana balance`:

```bash
saldo de solana <ACCOUNT_ADDRESS>
```
