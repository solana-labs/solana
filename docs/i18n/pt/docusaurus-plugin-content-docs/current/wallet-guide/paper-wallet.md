---
title: Paper Wallet
---

Este documento descreve como criar e usar uma carteira de papel com as ferramentas CLI Solana.

> Não pretendemos aconselhar sobre como _criar ou gerenciar carteiras em papel de forma segura._. Por favor, pesquise as preocupações de segurança com cuidado.

## Geral

Solana fornece uma ferramenta chave de geração para derivar as chaves de frases BIP39 em conformidade com conformidade. Solana CLI comandos para executar um validador e staking tokens todos suportam entrada de keypair via frases de sementes.

Para saber mais sobre o padrão BIP39, visite o repositório [do Bitcoin BIPs aqui](https://github.com/bitcoin/bips/blob/master/bip-0039.mediawiki).

## Utilização de Paper Wallet

Os comandos do Solana podem ser executados sem nunca salvar um par de chaves para o disco em uma máquina. Se evitar escrever uma chave privada para o disco é uma preocupação de segurança sua, você veio ao lugar certo.

> Mesmo usando esse método de entrada seguro, ainda é possível que uma chave privada seja escrita em disco por troca de memória não criptografada. É responsabilidade do utilizador proteger-se contra este cenário.

## Antes de Começar

- [Instale as ferramentas de linha de comando Solana](../cli/install-solana-cli-tools.md)

### Verifique a sua instalação

Verifique se o `solana-keygen` está instalado corretamente executando:

```bash
solana-keyen --version
```

## Criando uma Paper Wallet

Usando a ferramenta `solana-keygen`, é possível gerar novas semente de frases, bem como derivar um par de chaves de uma seed phrase existente e (opcional) senha. A semente e a senha podem ser usadas juntas como carteira de papel. Enquanto mantiver sua seed phrase e sua senha armazenadas de forma segura, você pode usá-los para acessar sua conta.

> Para obter mais informações sobre como as frases de semente funcionam, revise esta [página Bitcoin Wiki](https://en.bitcoin.it/wiki/Seed_phrase).

### Geração da frase semente

Gerar um novo par de chaves pode ser feito usando o comando `solana-keygen nova`. O comando irá gerar uma frase de semente aleatória, peça que você digite uma frase-passe opcional, e então exibirá a chave pública derivada e a semente gerada para a sua carteira papel.

Após copiar sua frase de semente, você pode usar as [instruções de derivação de chave pública](#public-key-derivation) para verificar que você não cometeu quaisquer erros.

```bash
solana-keygen novo --no-outfile
```

> Se o sinalizador `--no-outfile` for **omitido**, o comportamento padrão é escrever o par de chaves para `~/. onfig/solana/id.json`, resultando em uma [carteira de sistema de arquivo](file-system-wallet.md)

A saída deste comando mostrará uma linha como esta:

```bash
pubkey: 9ZNTfG4NyQgxy2SWjSiQoUyBPEvXT2xo7fKc5hPYYJ7b
```

O valor mostrado após `pubkey:` é o seu _endereço de carteira_.

**Nota:** Ao trabalhar com paper wallets e carteiras de sistema de arquivos, os termos "pubkey" e "endereço da carteira" são, às vezes, usados intermutavelmente.

> Para aumentar a segurança, aumenta a contagem de palavras da seed usando o argumento `--word-count`

Para obter detalhes de uso completo, execute:

```bash
solana-keygen novo --no-outfile
```

### Derivação de Chave Pública

Chaves públicas podem ser derivadas de uma semente de uma frase secreta e uma senha se você escolher usar uma. This is useful for using an offline-generated seed phrase to derive a valid public key. The `solana-keygen pubkey` command will walk you through how to use your seed phrase (and a passphrase if you chose to use one) as a signer with the solana command-line tools using the `ask` uri scheme.

```bash
solana-keygen pubkey prompt://
```

> Observe que você pode usar frases secretas diferentes para a mesma frase de semente. Cada senha única dará um par de chaves diferente.

A ferramenta `solana-keygen` usa a mesma lista padrão BIP39 em inglês que ela gera semente de frases. Se sua frase de sementes foi gerada por outra ferramenta que usa uma lista diferente de palavras, você ainda pode usar `solana-keygen`, mas precisará passar o argumento de `--skip-seed-phrase-validation` e ignorar essa validação.

```bash
solana-keygen pubkey prompt:// --skip-seed-phrase-validation
```

After entering your seed phrase with `solana-keygen pubkey prompt://` the console will display a string of base-58 character. This is the base _wallet address_ associated with your seed phrase.

> Copie o endereço derivado para um pendrive USB para fácil uso em computadores de rede

> Um próximo passo comum é [verificar o saldo](#checking-account-balance) da conta associada a uma chave pública

Para obter detalhes de uso completo, execute:

```bash
solana-keygen novo --no-outfile
```

### Hierarchical Derivation

The solana-cli supports [BIP32](https://github.com/bitcoin/bips/blob/master/bip-0032.mediawiki) and [BIP44](https://github.com/bitcoin/bips/blob/master/bip-0044.mediawiki) hierarchical derivation of private keys from your seed phrase and passphrase by adding either the `?key=` query string or the `?full-path=` query string.

By default, `prompt:` will derive solana's base derivation path `m/44'/501'`. To derive a child key, supply the `?key=<ACCOUNT>/<CHANGE>` query string.

```bash
solana-keygen pubkey prompt://?key=0/1
```

To use a derivation path other than solana's standard BIP44, you can supply `?full-path=m/<PURPOSE>/<COIN_TYPE>/<ACCOUNT>/<CHANGE>`.

```bash
solana-keygen pubkey prompt://?full-path=m/44/2017/0/1
```

Because Solana uses Ed25519 keypairs, as per [SLIP-0010](https://github.com/satoshilabs/slips/blob/master/slip-0010.md) all derivation-path indexes will be promoted to hardened indexes -- eg. `?key=0'/0'`, `?full-path=m/44'/2017'/0'/1'` -- regardless of whether ticks are included in the query-string input.

## Verificando o par de chaves

Para verificar que você controla a chave privada de um endereço de carteira de papel, use `verificações de solana-keygen`:

```bash
solana-keygen verify <PUBKEY> prompt://
```

where `<PUBKEY>` is replaced with the wallet address and the keyword `prompt://` tells the command to prompt you for the keypair's seed phrase; `key` and `full-path` query-strings accepted. Note that for security reasons, your seed phrase will not be displayed as you type. After entering your seed phrase, the command will output "Success" if the given public key matches the keypair generated from your seed phrase, and "Failed" otherwise.

## Verificando Saldo de uma Conta

Tudo que é necessário para verificar o saldo da conta é a chave pública de uma conta. Para recuperar as chaves públicas de forma segura a partir de uma carteira de papel, siga as instruções de [Derivação da chave pública](#public-key-derivation) em um [computador preso](<https://en.wikipedia.org/wiki/Air_gap_(networking)>). Chaves públicas podem ser digitadas manualmente ou transferidas através de um USB para uma máquina em rede.

Em seguida, configure a ferramenta de `solana` CLI para [conectar a um determinado cluster](../cli/choose-a-cluster.md):

```bash
configuração solana set --url <CLUSTER URL> # (ou seja, https://api.mainnet-beta.solana.com)
```

Finalmente, para verificar o saldo, execute o seguinte comando:

```bash
saldo de solana <PUBKEY>
```

## Criando Vários Endereços com Carteira em Papel

Você pode criar quantos endereços de carteira quiser. Simplesmente execute novamente os passos de em [Geração de Frases Sementes](#seed-phrase-generation) ou [Derivação de Chave Pública](#public-key-derivation) para criar um novo endereço. Vários endereços de carteira podem ser úteis se você deseja transferir tokens entre suas próprias contas para diferentes fins.

## Suporte

Confira nossa [página de suporte para Wallet](support.md) para ver maneiras de obter ajuda.
