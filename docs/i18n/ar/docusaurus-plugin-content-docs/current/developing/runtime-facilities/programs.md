---
title: "Native Programs"
---

Solana contains a small handful of native programs, which are required to run validator nodes. Unlike third-party programs, the native programs are part of the validator implementation and can be upgraded as part of cluster upgrades. قد تحدث الترقيات لإضافة ميزات أو إصلاح الأخطاء أو تحسين الأداء. نادرًا ما تحدث تغييرات في الواجهة للتعليمات الفردية. بدلاً من ذلك، عند الحاجة إلى التغيير، تتم إضافة إرشادات جديدة ويتم وضع علامة على الإرشادات السابقة كمُتَجاهلة. يُمكن ترقية التطبيقات وفقًا للجدول الزمني الخاص بها دون القلق من حدوث أعطال أثناء الترقيات.

For each native program the program id and description each supported instruction is provided. A transaction can mix and match instructions from different programs, as well include instructions from on-chain programs.

## برنامج النظام (System Program)

Create new accounts, allocate account data, assign accounts to owning programs, transfer lamports from System Program owned accounts and pay transacation fees.

- مُعرف البرنامج: `11111111111111111111111111111111111111111111`
- التعليمات: تعليمات النظام [SystemInstruction](https://docs.rs/solana-sdk/VERSION_FOR_DOCS_RS/solana_sdk/system_instruction/enum.SystemInstruction.html)

## إعدادات البرنامج (Config Program)

إضافة الإعدادات إلى الشبكة وقائمة المفاتيح العمومية (public keys) المسموح لها بتعديلها

- مُعرف البرنامج: `11111111111111111111111111111111111111111111`
- التعليمات: إعدادات النظام: [config_instruction](https://docs.rs/solana-config-program/VERSION_FOR_DOCS_RS/solana_config_program/config_instruction/index.html)

على خلاف البرامج الأخرى، لا تحدد إعدادات البرنامج (Config Program) أي تعليمات فردية. يحتوي على تعليمات ضمنية واحدة فقط، تعليمات "مخزن" ("store"). بيانات التعليمات الخاصة به هي مجموعة من المفاتيح التي تسمح بالوصول إلى الحساب والبيانات لتخزينها فيه.

## برنامج إثبات الحِصَّة أو التَّحْصِيص (Stake Program)

Create and manage accounts representing stake and rewards for delegations to validators.

- مُعرف البرنامج: `11111111111111111111111111111111111111111111`
- التعليمات: تعليمات إثبات الحِصَّة أو التَّحْصِيص [StakeInstruction](https://docs.rs/solana-stake-program/VERSION_FOR_DOCS_RS/solana_stake_program/stake_instruction/enum.StakeInstruction.html)

## برنامج التصويت (Vote Program)

Create and manage accounts that track validator voting state and rewards.

- مُعرف البرنامج: `11111111111111111111111111111111111111111111`
- التعليمات: تعليمات التصويت [VoteInstruction](https://docs.rs/solana-vote-program/VERSION_FOR_DOCS_RS/solana_vote_program/vote_instruction/enum.VoteInstruction.html)

## مُحمِّل BPF أو (BPF Loader)

Deploys, upgrades, and executes programs on the chain.

- Program id: `BPFLoaderUpgradeab1e11111111111111111111111`
- التعليمات: تعليمات المُحمِّل [LoaderInstruction](https://docs.rs/solana-sdk/VERSION_FOR_DOCS_RS/solana_sdk/loader_upgradeable_instruction/enum.UpgradeableLoaderInstruction.html)

The BPF Upgradeable Loader marks itself as "owner" of the executable and program-data accounts it creates to store your program. When a user invokes an instruction via a program id, the Solana runtime will load both your the program and its owner, the BPF Upgradeable Loader. The runtime then passes your program to the BPF Upgradeable Loader to process the instruction.

[More information about deployment](cli/deploy-a-program.md)

## برنامج Secp256k1

تحقق من عمليات إستعادة المفتاح العمومي (public key) المُسَمَّى secp256k1 عن طريق (ecrecover).

- مُعرف البرنامج: `11111111111111111111111111111111111111111111`
- التعليمات: [new_secp256k1_instruction](https://github.com/solana-labs/solana/blob/1a658c7f31e1e0d2d39d9efbc0e929350e2c2bcb/sdk/src/secp256k1_instruction.rs#L31)

يُعالج برنامج secp256k1 تعليمة (instruction) تأخذ في إعتبارها الـ byte الأول كبداية تعداد من البنية التالية المُتسلسلة في بيانات التعليمات:

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

كود رمز العملية (Pseudo code of the operation):

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

يسمح هذا للمُستخدم بتحديد أي بيانات تعليمات في المُعاملة للتوقيع وبيانات الرسائل. عن طريق تحديد نظام التعليمات الخاصة، يمكن للمرء أيضا أن يتلقى البيانات من المُعاملة نفسها. من خلال تحديد تعليمات sysvar خاصة، يُمكن أيضا تلقي بيانات من المُعاملة نفسها.

تكلفة المُعاملة سوف تحسب عدد توقيعات التَحَقُّق ضارب مُضاعفة تكلفة التوقيع للتَحَقُّق.

### مُلاحظات التحسين (Optimization notes)

يجب أن تتم العملية بعد إلغاء التسلسل (جزئيًا على الأقل)، ولكن جميع المُدخلات (entries) تأتي من بيانات المُعاملة نفسها، مما يتيح سهولة التنفيذ نسبيًا بالتوازي مع مُعالجة المُعاملات والتحقق من الـ PoH.
