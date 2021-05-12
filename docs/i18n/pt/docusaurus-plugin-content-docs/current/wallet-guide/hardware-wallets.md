---
title: Usando Carteiras de Hardware com o CLI Solana
---

A assinatura de uma transação requer uma chave privada, mas armazenar uma chave privada em seu computador pessoal ou telefone a deixa sujeita a roubo. Adicionar uma senha a sua chave adiciona segurança, mas muitas pessoas preferem ir mais longe e mover suas chaves privadas para um dispositivo físico separado chamado de _carteira de hardware_. Uma carteira de hardware é um dispositivo portátil que armazena chaves privadas e fornece interface para assinar transações.

O Solana CLI tem suporte de primeira classe para carteiras de hardware. Em qualquer lugar você usa um arquivo de par de chaves (denotado como `<KEYPAIR>` na documentação de uso), pode passar uma _URL de par de chaves_ que identifica exclusivamente um par de chaves em uma carteira de hardware.

## Carteiras de Hardware Suportadas

O Solana CLI suporta as seguintes carteiras de hardware:

- [Ledger Nano S e Ledger Nano X](hardware-wallets/ledger.md)

## Especifique uma URL do Par Chaves

Javier Solana define um formato de URL para parear exclusivamente qualquer keypair Solana em uma carteira de hardware conectada ao seu computador.

A URL do par de chaves tem o seguinte formulário, onde colchetes quadrados denotam campos opcionais:

```text
usb://<MANUFACTURER>[/<WALLET_ID>][?key=<DERIVATION_PATH>]
```

`WALLET_ID` é uma chave única globalmente usada para desambiguar vários dispositivos.

`DERVIATION_PATH` é usado para navegar até chaves Solana dentro de sua carteira de hardware. O caminho tem o formulário `<ACCOUNT>[/<CHANGE>]`, onde cada `CONTA` e `ALTERAR` são inteiros positivos.

Por exemplo, uma URL completa para um dispositivo Ledger pode ser:

```text
usb://ledger/BsNsvfXqQTttJnagwFWdBS7FBXgnsK8VZ5CmuznN85swK?key=0/0
```

Todos os caminhos de derivação incluem implicitamente o prefixo `44'/501'`,, que indica o caminho segue a [especificações BIP44](https://github.com/bitcoin/bips/blob/master/bip-0044.mediawiki) e que todas as chaves derivadas são chaves Solana (o tipo de moeda 501). A aspa única indica uma derivação "endurecida". Como Solana usa os pares chaves Ed25519, todas as derivadas são endurecidas e, portanto, adicionar a cotação é opcional e é desnecessário.
