---
title: "Native Programs"
---

Solana contains a small handful of native programs, which are required to run validator nodes. Unlike third-party programs, the native programs are part of the validator implementation and can be upgraded as part of cluster upgrades. 업그레이드는 기능 추가, 버그 픽스, 또는 퍼포먼스 개선을 위해 발생할 수 있습니다. 개별 명령에 대한 인터페이스 변경은 없을 것이며, 있다 해도 매우 드물게 일어날 것입니다. 대신, 변경이 필요하다면, 신규 명령이 추가되고 나서 이전의 것들은 지난 것으로서 마킹될 것입니다. 앱들은 업그레이드 간 대미지에 대한 걱정 없이 각자의 타임라인에 맞춰 업그레이드 할 수 있습니다.

For each native program the program id and description each supported instruction is provided. A transaction can mix and match instructions from different programs, as well include instructions from on-chain programs.

## 시스템 프로그램

Create new accounts, allocate account data, assign accounts to owning programs, transfer lamports from System Program owned accounts and pay transacation fees.

- 프로그램 id: `11111111111111111111111111111111`
- 명령: [SystemInstruction](https://docs.rs/solana-sdk/VERSION_FOR_DOCS_RS/solana_sdk/system_명령/enum.SystemInstruction.html)

## Config 프로그램

설정 데이터와 이를 변경할 수 있는 권한을 지닌 공개키 리스트를 체인에 추가할 수 있습니다.

- 프로그램 id: `Config1111111111111111111111111111111111111`
- 명령: [config_instruction](https://docs.rs/solana-config-program/VERSION_FOR_DOCS_RS/solana_config_program/config_명령/index.html)

다른 프로그램들과 달리 Config 프로그램은 그 어떠한 개별 명령도 정의할 수 없습니다. 다만 유일하게 "store"란 암시적 명령을 가지고 있습니다. 해당 명령 데이터는 계정과 계정에 저장된 데이터로 접근하는 키들의 세트 입니다.

## Stake 프로그램

Create and manage accounts representing stake and rewards for delegations to validators.

- 프로그램 id: `Stake11111111111111111111111111111111111111`
- 명령: [StakeInstruction](https://docs.rs/solana-stake-program/VERSION_FOR_DOCS_RS/solana_stake_program/stake_instruction/enum.StakeInstruction.html)

## Vote 프로그램

Create and manage accounts that track validator voting state and rewards.

- 프로그램 id: `Vote111111111111111111111111111111111111111`
- 명령: [VoteInstruction](https://docs.rs/solana-vote-program/VERSION_FOR_DOCS_RS/solana_vote_program/vote_instruction/enum.VoteInstruction.html)

## BPF Loader

Deploys, upgrades, and executes programs on the chain.

- Program id: `BPFLoaderUpgradeab1e11111111111111111111111`
- 명령: [LoaderInstruction](https://docs.rs/solana-sdk/VERSION_FOR_DOCS_RS/solana_sdk/loader_upgradeable_instruction/enum.UpgradeableLoaderInstruction.html)

The BPF Upgradeable Loader marks itself as "owner" of the executable and program-data accounts it creates to store your program. When a user invokes an instruction via a program id, the Solana runtime will load both your the program and its owner, the BPF Upgradeable Loader. The runtime then passes your program to the BPF Upgradeable Loader to process the instruction.

[More information about deployment](cli/deploy-a-program.md)

## Secp256k1 프로그램

secp256k1 공개키 복원 절차 검증 (ecrecover).

- 프로그램 id: `KeccakSecp256k11111111111111111111111111111`
- 명령: [new_secp256k1_instruction](https://github.com/solana-labs/solana/blob/1a658c7f31e1e0d2d39d9efbc0e929350e2c2bcb/sdk/src/secp256k1_instruction.rs#L31)

secp256k1 프로그램은 명령 데이터로 연속되는 다음의 구조 형태를 띈 명령을 처리합니다.

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

Pseudo code of the operation:

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

이를 통해 유저는 서명 및 메시지 데이터에 대한 트랜잭션 내 어떠한 명령 데이터라도 특정할 수 있습니다. sysvar의 특수한 명령을 특정함으로서 유저는 트랜잭션 자체로부터 데이터를 받을 수 있습니다.

트랜잭션 비용은 검증에 필요한 서명 수에 서명 검증 비용 배수의 곱입니다.

### Optimization notes

연산은 비연속화가 부분적이라도 진행된 뒤에 진행될 수 있지만, 모든 입력값은 트랜잭션 데이터로부터 나오고, 덕분에 실행에 있어 트랜잭션 처리와 역사증명 검증과 평행으로 처리되기 비교적 쉽게 됩니다.
