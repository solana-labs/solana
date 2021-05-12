---
title: "Native Programs"
---

Solana contains a small handful of native programs, which are required to run validator nodes. Unlike third-party programs, the native programs are part of the validator implementation and can be upgraded as part of cluster upgrades. Las actualizaciones pueden ocurrir para agregar características, corregir errores o mejorar el rendimiento. La interfaz cambia a instrucciones individuales rara vez, si es que alguna vez ocurre. En su lugar, cuando se necesita un cambio, se añaden nuevas instrucciones y las anteriores se marcan en desuso. Las aplicaciones pueden mejorar en su propia línea de tiempo sin preocuparse de averías a través de mejoras.

For each native program the program id and description each supported instruction is provided. A transaction can mix and match instructions from different programs, as well include instructions from on-chain programs.

## Programa del sistema

Create new accounts, allocate account data, assign accounts to owning programs, transfer lamports from System Program owned accounts and pay transacation fees.

- Id del programa: `111111111111111111111111111111111111`
- Instrucciones: [Instrucción del Sistema](https://docs.rs/solana-sdk/VERSION_FOR_DOCS_RS/solana_sdk/system_instruction/enum.SystemInstruction.html)

## Configurar programa

Añadir datos de configuración a la cadena y la lista de claves públicas que están permitidas para modificarla

- Id del programa: `Config1111111111111111111111111111111111111`
- Instrucciones: [config_instruction](https://docs.rs/solana-config-program/VERSION_FOR_DOCS_RS/solana_config_program/config_instruction/index.html)

A diferencia de los otros programas, el programa Config no define instrucciones individuales. Tiene sólo una instrucción implícita, una instrucción de "tienda". Sus datos de instrucciones son un conjunto de claves que puerta el acceso a la cuenta, y los datos para almacenar en ella.

## Programa Stake

Create and manage accounts representing stake and rewards for delegations to validators.

- Id del programa: `Stake111111111111111111111111111111111111111111`
- Instrucciones: [StakeInstruction](https://docs.rs/solana-stake-program/VERSION_FOR_DOCS_RS/solana_stake_program/stake_instruction/enum.StakeInstruction.html)

## Programa de votos

Create and manage accounts that track validator voting state and rewards.

- Id del programa: `Vote11111111111111111111111111111111111111111`
- Instrucciones: [VoteInstruction](https://docs.rs/solana-vote-program/VERSION_FOR_DOCS_RS/solana_vote_program/vote_instruction/enum.VoteInstruction.html)

## Cargador BPF

Deploys, upgrades, and executes programs on the chain.

- Program id: `BPFLoaderUpgradeab1e11111111111111111111111`
- Instrucciones: [LoaderInstruction](https://docs.rs/solana-sdk/VERSION_FOR_DOCS_RS/solana_sdk/loader_upgradeable_instruction/enum.UpgradeableLoaderInstruction.html)

The BPF Upgradeable Loader marks itself as "owner" of the executable and program-data accounts it creates to store your program. When a user invokes an instruction via a program id, the Solana runtime will load both your the program and its owner, the BPF Upgradeable Loader. The runtime then passes your program to the BPF Upgradeable Loader to process the instruction.

[More information about deployment](cli/deploy-a-program.md)

## Programa Secp256k1

Verifique las operaciones de recuperación de claves públicas secp256k1 (ecrecover).

- Id del programa: `KeccakSecp256k111111111111111111111111111111111`
- Instrucciones: [new_secp256k1_instruction](https://github.com/solana-labs/solana/blob/1a658c7f31e1e0d2d39d9efbc0e929350e2c2bcb/sdk/src/secp256k1_instruction.rs#L31)

El programa secp256k1 procesa una instrucción que toma como el primer byte un recuento de la siguiente estructura serializada en los datos de instrucción:

```
struct Secp256k1SignatureOffsets {
    secp_signature_key_offset: u16,        // offset to [signature,recovery_id,etherum_address] of 64+1+20 bytes
    secp_signature_instruction_index: u8,  // instruction index to find data
    secp_pubkey_offset: u16,               // offset to [signature,recovery_id] of 64+1 bytes
    secp_signature_instruction_index: u8,  // instruction index to find data
    secp_message_data_offset: u16,         // offset to start of message data
    secp_message_data_size: u16,           // size of message data
    secp_message_instruction_index: u8,    // index of instruction data to get message data
}
```

Pseudocódigo de la operación:

```
process_instruction() {
  for i in 0..count {
      // i'th index values referenced:
      instructions = &transaction.message().instructions
      signature = instructions[secp_signature_instruction_index].data[secp_signature_offset..secp_signature_offset + 64]
      recovery_id = instructions[secp_signature_instruction_index].data[secp_signature_offset + 64]
      ref_eth_pubkey = instructions[secp_pubkey_instruction_index].data[secp_pubkey_offset..secp_pubkey_offset + 32]
      message_hash = keccak256(instructions[secp_message_instruction_index].data[secp_message_data_offset..secp_message_data_offset + secp_message_data_size])
      pubkey = ecrecover(signature, recovery_id, message_hash)
      eth_pubkey = keccak256(pubkey[1..])[12..]
      if eth_pubkey != ref_eth_pubkey {
          return Error
      }
  }
  return Success
}
```

Esto permite al usuario especificar cualquier dato de instrucción en la transacción para datos de firma y mensaje. Al especificar una instrucción especial sysvar, uno puede también recibir datos de la propia transacción.

El coste de la transacción contará el número de firmas a verificar multiplicado por el multiplicador del coste de verificación de la firma.

### Notas de optimización

La operación tendrá lugar después de (al menos parcial) deserialización, pero todas las entradas provienen de los datos de la transacción en sí. esto le permite ser relativamente fácil de ejecutar en paralelo al procesamiento de transacciones y verificación de PoH.
