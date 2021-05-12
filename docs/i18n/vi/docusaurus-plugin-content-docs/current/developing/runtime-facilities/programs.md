---
title: "Native Programs"
---

Solana contains a small handful of native programs, which are required to run validator nodes. Unlike third-party programs, the native programs are part of the validator implementation and can be upgraded as part of cluster upgrades. Có thể nâng cấp để thêm tính năng, sửa lỗi hoặc cải thiện hiệu suất. Các thay đổi giao diện đối với các hướng dẫn riêng lẻ sẽ hiếm khi xảy ra, nếu có. Thay vào đó, khi cần thay đổi, các hướng dẫn mới sẽ được thêm vào và các hướng dẫn trước đó được đánh dấu là không dùng nữa. Các ứng dụng có thể nâng cấp theo tiến trình của riêng chúng mà không lo bị hỏng khi nâng cấp.

For each native program the program id and description each supported instruction is provided. A transaction can mix and match instructions from different programs, as well include instructions from on-chain programs.

## Chương trình Hệ thống

Create new accounts, allocate account data, assign accounts to owning programs, transfer lamports from System Program owned accounts and pay transacation fees.

- Id chương trình: `11111111111111111111111111111111`
- Hướng dẫn: [SystemInstruction](https://docs.rs/solana-sdk/VERSION_FOR_DOCS_RS/solana_sdk/system_instruction/enum.SystemInstruction.html)

## Chương trình Cấu hình

Thêm dữ liệu cấu hình vào chuỗi và danh sách các public key được phép sửa đổi nó

- Id chương trình: `Config1111111111111111111111111111111111111`
- Hướng dẫn: [config_instruction](https://docs.rs/solana-config-program/VERSION_FOR_DOCS_RS/solana_config_program/config_instruction/index.html)

Không giống như các chương trình khác, Chương trình cấu hình không xác định bất kỳ hướng dẫn riêng lẻ nào. Nó chỉ có một chỉ dẫn ngầm, một chỉ dẫn "cửa hàng". Dữ liệu hướng dẫn của nó là một tập hợp các khóa để truy cập vào tài khoản và dữ liệu để lưu trữ trong đó.

## Chương trình Stake

Create and manage accounts representing stake and rewards for delegations to validators.

- Id chương trình: `Stake11111111111111111111111111111111111111`
- Hướng dẫn: [StakeInstruction](https://docs.rs/solana-stake-program/VERSION_FOR_DOCS_RS/solana_stake_program/stake_instruction/enum.StakeInstruction.html)

## Chương trình Bình Chọn

Create and manage accounts that track validator voting state and rewards.

- Id chương trình: `Vote111111111111111111111111111111111111111`
- Hướng dẫn: [VoteInstruction](https://docs.rs/solana-vote-program/VERSION_FOR_DOCS_RS/solana_vote_program/vote_instruction/enum.VoteInstruction.html)

## Bộ nạp BPF

Deploys, upgrades, and executes programs on the chain.

- Program id: `BPFLoaderUpgradeab1e11111111111111111111111`
- Hướng dẫn: [LoaderInstruction](https://docs.rs/solana-sdk/VERSION_FOR_DOCS_RS/solana_sdk/loader_upgradeable_instruction/enum.UpgradeableLoaderInstruction.html)

The BPF Upgradeable Loader marks itself as "owner" of the executable and program-data accounts it creates to store your program. When a user invokes an instruction via a program id, the Solana runtime will load both your the program and its owner, the BPF Upgradeable Loader. The runtime then passes your program to the BPF Upgradeable Loader to process the instruction.

[More information about deployment](cli/deploy-a-program.md)

## Chương trình Secp256k1

Xác minh hoạt động khôi phục public key secp256k1 (ecrecover).

- Id chương trình: `KeccakSecp256k11111111111111111111111111111`
- Hướng dẫn: [new_secp256k1_instruction](https://github.com/solana-labs/solana/blob/1a658c7f31e1e0d2d39d9efbc0e929350e2c2bcb/sdk/src/secp256k1_instruction.rs#L31)

Chương trình secp256k1 xử lý một lệnh lấy byte đầu tiên đếm số cấu trúc sau được tuần tự hóa trong dữ liệu chỉ dẫn:

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

Code giả của hoạt động:

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

Điều này cho phép người dùng chỉ định bất kỳ dữ liệu hướng dẫn nào trong giao dịch cho chữ ký và dữ liệu tin nhắn. Bằng cách chỉ định một sysvar hướng dẫn đặc biệt, người ta cũng có thể nhận dữ liệu từ chính giao dịch.

Chi phí của giao dịch sẽ tính số chữ ký cần xác minh nhân với hệ số nhân chi phí xác minh chữ ký.

### Ghi chú tối ưu hóa

Hoạt động sẽ phải diễn ra sau (ít nhất là một phần) deserialization, nhưng tất cả các đầu vào đều đến từ chính dữ liệu giao dịch, điều này cho phép nó tương đối dễ thực hiện song song với xử lý giao dịch và xác minh PoH.
