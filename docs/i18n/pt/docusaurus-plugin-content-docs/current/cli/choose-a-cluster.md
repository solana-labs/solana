---
title: Conectando-se a um cluster
---

Veja [Solana Clusters](../clusters.md) para informações gerais sobre os clusters disponíveis.

## Configure a ferramenta de linha de comando

Você pode verificar qual cluster a ferramenta de linha de comando Solana (CLI) está sendo direcionada executando o seguinte comando:

```bash
solana config get
```

Use a configuração do `solana define o comando` para direcionar um determinado cluster. Depois de definir um alvo de agrupamento, quaisquer subcomandos futuros enviar/receber informações daquele cluster.

Por exemplo, para direcionar o cluster Devnet, executar:

```bash
configuração solana set --url # (ou seja, https://devnet.solana.com
```

## Assegurar que as versões correspondam

Embora não seja estritamente necessário, o CLI geralmente funcionará melhor quando sua versão corresponder à versão do software em execução no cluster. Para obter a versão do CLI instalada, execute:

```bash
solana --version
```

Para obter a versão do cluster execute:

```bash
versão solana cluster-version
```

Certifique-se de que a versão local do CLI seja maior ou igual à versão do cluster.
