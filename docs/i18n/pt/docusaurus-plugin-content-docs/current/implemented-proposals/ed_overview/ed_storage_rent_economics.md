---
title: Economia de aluguel de armazenamento
---

Cada transação que é enviada ao livro razão Javier Solana impõe custos. Taxas de transação pagas pelo remetente e coletadas por um validador, em teoria, conta pelos custos agudos, transacionais, de validação e adição desses dados ao livro razão. Descontabilizado nesse processo é o armazenamento intercalar do estado de livro razão ativo, necessariamente mantido pelo validador rotativo definido. Este tipo de armazenamento impõe custos não só aos validadores, mas também à rede em geral, à medida que o estado ativo aumenta, assim como a transmissão e validação de dados. Para explicar estes custos, descrevemos aqui a nossa concepção preliminar e a implementação das rendas de armazenamento.

Aluguel de armazenamento pode ser pago através de um dos dois métodos:

Método 1: Defina e esqueça

Com esta abordagem, contas com depósitos garantidos no valor de dois anos estão isentas das taxas de renda da rede. Mantendo este saldo mínimo, os benefícios mais amplos da rede de redução de liquidez e o titular da conta pode confiar que sua `Account::data` será mantida para acesso/uso contínuo.

Método 2: Pagar por byte

Se uma conta tem menos de dois anos de depósito depositado, as taxas de rede são cobradas aluguel por período, em crédito para a próxima época. Este aluguel é deduzido a uma taxa especificada em genesis, em lâmpados por kilobyte-ano.

Para obter informações sobre os detalhes de implementação técnica deste design, consulte a seção [Aluguel](../rent.md).
