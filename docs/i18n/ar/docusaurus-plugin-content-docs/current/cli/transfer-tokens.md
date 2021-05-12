---
title: إرسال واستقبال الرموز المميزة
---

تصف هذه الصفحة كيفية إستقبال وإرسال رموز SOL بإستخدام أدوات سطر الأوامر (command-line tools) مع محفظة سطر الأوامر (command line wallet) مثل المحفظة الورقية [paper wallet](../wallet-guide/paper-wallet.md)، محفظة ملفات النظام [file system wallet](../wallet-guide/file-system-wallet.md) أو المحفظة الخارجية [hardware wallet](../wallet-guide/hardware-wallets.md). قبل أن تبدأ، تأكد من أنك قُمت بإنشاء محفظة ولديك حق الوصول إلى عنوانها المفتاح العمومي (pubkey) وزوج المفاتيح (keypair) المُوَقِّع. تحقق من قواعدنا الخاصة بإدخال زوج المفاتيح (keypair) لأنواع المحافظ المُختلفة [conventions for entering keypairs for different wallet types](../cli/conventions.md#keypair-conventions).

## إختبار محفظتك (Testing your Wallet)

قبل مُشاركة المفتاح العمومي (public key) الخاص بك مع الآخرين، قد ترغب أولا في التأكد من أن المفتاح صالح وأنه بالفعل لديك المفتاح الخاص (private key) المُناسب.

في هذا المثال، سنقوم بإنشاء محفظة ثانية بالإضافة إلى محفظتك الأولى، ثم نقل بعض الرموز إليها. هذا سيؤكد أنه يُمكنك إرسال وإستلام الرموز على نوع المحفظة الخاص بك.

يستخدم مثال الإختبار هذا شبكة المُطوِّرين التجريبية (Developer Testnet) الخاصة بنا، والتي تُسَمَّى devnet. الرموز المُصدرة على شبكة المُطوِّرين (Devnet) ليس لديها **no** قيمة، لذا لا تقلق إذا فقدتها.

#### أرسل بعض الرموز بتوزيع حر (Airdrop) للبدء

أولا، قُم بإرسال بتوزيع حر _airdrop_ لنفسك على شبكة المُطوِّرين (Devnet).

```bash
solana airdrop 10 <RECIPIENT_ACCOUNT_ADDRESS> --url https://devnet.solana.com
```

حيث تستبدل النص `<RECIPIENT_ACCOUNT_ADDRESS>` المفتاح العمومي (public key) / عنوان المحفظة (wallet address) المُرمّز base58.

#### تحقق من رصيدك

تأكد من نجاح التوزيع الحر (airdrop) بالتَحَقُّق من رصيد الحساب. يجب أن تكون النتيجة `10 SOL`:

```bash
رصيد solana <ACCOUNT_ADDRESS> --url https://devnet.solana.com
```

#### إنشاء عنوان محفظة ثان

سنحتاج إلى عنوان جديد لإستلام الرموز الخاصة بنا. أنشئ زوج مفاتيح (keypair) ثان وسَجِّل المفتاح العمومي (pubkey) الخاص به:

```bash
solana-keygen new --no-passphrase --no-outfile
```

سيحتوي المُخرَج (output) على العنوان بعد النص `pubkey:`. إنسخ العنوان. سوف نستخدمه في الخطوة التالية.

```text
pubkey: GKvqsuNcnwWqPzzuhLmGi4rzzh55FhJtGizkhHaEJqiV
```

يُمكنك أيضا إنشاء محفظة ثانية (أو أكثر) من أي نوع: محفظة ورقية [paper](../wallet-guide/paper-wallet#creating-multiple-paper-wallet-addresses) ، محفظة ملفات نظام [file system](../wallet-guide/file-system-wallet.md#creating-multiple-file-system-wallet-addresses) أو محفظة خارجية [hardware](../wallet-guide/hardware-wallets.md#multiple-addresses-on-a-single-hardware-wallet).

#### نقل الرموز من محفظتك الأولى إلى العنوان الثاني

بعد ذلك، أثبت أنك تملك الرموز التي تم إرسالها بالتوزيع الحر (airdropped) وذلك عن طريق نقلها. لن تقبل مجموعة Solana التحويل إلا إذا قُمت بالتوقيع على المُعاملة بإستخدام زوج المفاتيح (keypair) الخاص المُطابق للمفتاح العمومي (public key) للقُرسَل إليه في القُعاملة.

```bash
solana transfer --from <KEYPAIR> <RECIPIENT_ACCOUNT_ADDRESS> 5 --url https://devnet.solana.com --fee-payer <KEYPAIR>
```

حيث تستبدل زوج المفاتيح `<KEYPAIR>` بالمسار إلى زوج المفاتيح (keypair) في محفظتك الأولى، وقُم بإستبدال عنوان حساب المُسْتَلِم `<RECIPIENT_ACCOUNT_ADDRESS>` بعنوان محفظتك الثانية.

قُم بتأكيد الأرصدة المُحدثة مع رصيد `solana balance`:

```bash
solana balance <ACCOUNT_ADDRESS> --url http://devnet.solana.com
```

حيث `<ACCOUNT_ADDRESS>` هو الpublic key من الkeypair الخاص بك أو الpublic key للمستلم.

#### مثال كامل على على تجربة الإرسال

```bash
$ solana-keygen new --outfile my_solana_wallet.json   # Creating my first wallet, a file system wallet
Generating a new keypair
For added security, enter a passphrase (empty for no passphrase):
Wrote new keypair to my_solana_wallet.json
==========================================================================
pubkey: DYw8jCTfwHNRJhhmFcbXvVDTqWMEVFBX6ZKUmG5CNSKK                          # Here is the address of the first wallet
==========================================================================
Save this seed phrase to recover your new keypair:
width enhance concert vacant ketchup eternal spy craft spy guard tag punch    # If this was a real wallet, never share these words on the internet like this!
==========================================================================

$ solana airdrop 10 DYw8jCTfwHNRJhhmFcbXvVDTqWMEVFBX6ZKUmG5CNSKK --url https://devnet.solana.com  # Airdropping 10 SOL to my wallet's address/pubkey
Requesting airdrop of 10 SOL from 35.233.193.70:9900
10 SOL

$ solana balance DYw8jCTfwHNRJhhmFcbXvVDTqWMEVFBX6ZKUmG5CNSKK --url https://devnet.solana.com # Check the address's balance
10 SOL

$ solana-keygen new --no-outfile  # Creating a second wallet, a paper wallet
Generating a new keypair
For added security, enter a passphrase (empty for no passphrase):
====================================================================
pubkey: 7S3P4HxJpyyigGzodYwHtCxZyUQe9JiBMHyRWXArAaKv                   # Here is the address of the second, paper, wallet.
====================================================================
Save this seed phrase to recover your new keypair:
clump panic cousin hurt coast charge engage fall eager urge win love   # If this was a real wallet, never share these words on the internet like this!
====================================================================

$ solana transfer --from my_solana_wallet.json 7S3P4HxJpyyigGzodYwHtCxZyUQe9JiBMHyRWXArAaKv 5 --url https://devnet.solana.com --fee-payer my_solana_wallet.json  # Transferring tokens to the public address of the paper wallet
3gmXvykAd1nCQQ7MjosaHLf69Xyaqyq1qw2eu1mgPyYXd5G4v1rihhg1CiRw35b9fHzcftGKKEu4mbUeXY2pEX2z  # This is the transaction signature

$ solana balance DYw8jCTfwHNRJhhmFcbXvVDTqWMEVFBX6ZKUmG5CNSKK --url https://devnet.solana.com
4.999995 SOL  # The sending account has slightly less than 5 SOL remaining due to the 0.000005 SOL transaction fee payment

$ solana balance 7S3P4HxJpyyigGzodYwHtCxZyUQe9JiBMHyRWXArAaKv --url https://devnet.solana.com
5 SOL  # The second wallet has now received the 5 SOL transfer from the first wallet

```

## إستقبال الرموز (Receive Tokens)

لإستقبال الرموز، ستحتاج إلى عنواين الآخرين لإرسال الرموز إليهم. في Solana، عنوان المحفظة هو المفتاح العمومي (public key) لزوج المفاتيح (keypair). هناك مجموعة مُتنوعة من التقنيات لإنشاء أو توليد زوج المفاتيح (keypair). تعتمد الطريقة التي تختارها على كيفية إختيارك لتخزين زوج المفاتيح (keypair). يتم تخزين زوج المفاتيح (keypair) في المحافظ. قبل إستلام الرموز، ستحتاج إلى إنشاء محفظة [create a wallet](../wallet-guide/cli.md). بمجرد إكتماله، يجب أن يكون لديك مفتاح عمومي (public key) لكل زوج مفاتيح (keypair) أنشأته. المفتاح العمومي (public key) هو سلسلة طويلة من رموز base58. يتراوح طولها من 32 إلى 44 رمزا.

## إرسال الرموز (Send Tokens)

إذا كنت تمتلك بالفعل رموز SOL وترغب في إرسال الرموز إلى شخص ما، فستحتاج إلى مسار لزوج المفاتيح (keypair) الخاص بك، والمفتاح العمومي (public key) المُرمّز بـ base58، وعدد من الرموز لإرسالها. بمجرد إنتهاءك من المرحلة السابقة، يُمكنك إرسال العملات الرموز مع أمر `solana transfer`:

```bash
solana transfer --from <KEYPAIR> <RECIPIENT_ACCOUNT_ADDRESS> <AMOUNT> --fee-payer <KEYPAIR>
```

قُم بتأكيد الأرصدة المُحَدَّثَة بواسطة `solana balance`:

```bash
solana balance <ACCOUNT_ADDRESS>
```
