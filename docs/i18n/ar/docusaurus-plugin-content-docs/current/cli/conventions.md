---
title: إستخدام أداة CLI الخاصة بSolana
---

قبل تشغيل أي أوامر بأداة CLI الخاصة بSolana، دعنا ننتقل إلى بعض الإصطلاحات التي ستراها عبر جميع الأوامر. أولا، إن أداة CLI الخاصة بSolana هي في الواقع عبارة عن مجموعة أوامر مختلفة لكل إجراء قد ترغب في إتخاذه. يمكنك عرض قائمة جميع الأوامر الممكنة عن طريق تشغيل:

```bash
solana --help
```

لتقريب الصورة أكثر حول كيفية إستخدام أمر معين، قم بتشغيل:

```bash
solana <COMMAND> --help
```

حيث تستبدل النص `<COMMAND>` بإسم الأمر الذي تريده لمعرفة المزيد عن ذلك.

عادة ما تحتوي رسالة إستخدام الأوامر على كلمات مثل `<AMOUNT>`، `<ACCOUNT_ADDRESS>` أو `<KEYPAIR>`. كل كلمة هي نائب عنصر نائب لنوع _type_ النص الذي يُمكنك تتنفيذ الأمر به. على سبيل المثال، يُمكنك إستبدال `<AMOUNT>` بعدد مثل `42` أو `100.42`. يُمكنك إستبدال `<ACCOUNT_ADDRESS>` بتشفير base58 للمفتاح العمومي (pubkey) الخاص بك، مثل `9grmKMwTiZwUHSExjtbFzHLPTdWoXcg1bZkhvwTrTww`.

## إصطلاحات زوج المفاتيح (Keypair conventions)

العديد من الأوامر التي تستخدم أدوات CLI تتطلب قيمة لـ `<KEYPAIR>`. القيمة التي يجب أن تستخدمها لزوج المفاتيح (keypair) تعتمد على نوع محفظة سطر الأوامر التي قُمت بإنشائها [command line wallet you created](../wallet-guide/cli.md).

على سبيل المثال، طريقة عرض أي عنوان محفظة (يعرف أيضا بإسم الـ pubkey الخاص بالـ keypair)، وثيقة المساعدة لأداة CLI تعرض ما يلي:

```bash
solana-keygen pubkey <KEYPAIR>
```

نعرض أدناه الحل لما يجب أن تضعه في `<KEYPAIR>` إعتمادا على نوع محفظتك.

#### المحفظة الورقية (Paper Wallet)

في المحفظة الورقية، يتم إشتقاق زوج المفاتيح (keypair) بشكل آمن من كلمات الإستِرداد (seed words) و عبارة كلمة المرور (passphrase) الإختيارية التي أدخلتها عند إنشاء المحفظة. To use a paper wallet keypair anywhere the `<KEYPAIR>` text is shown in examples or help documents, enter the uri scheme `prompt://` and the program will prompt you to enter your seed words when you run the command.

لعرض عنوان المحفظة لمحفظة ورقية (Paper Wallet):

```bash
solana-keygen pubkey prompt://
```

#### محفظة نظام الملفات (File System Wallet)

مع محفظة نظام الملفات، يتم تخزين زوج المفاتيح (keypair) في ملف على جهاز الكمبيوتر الخاص بك. إستبدل `<KEYPAIR>` بمسار الملف الكامل لملف زوج المفاتيح (keypair).

على سبيل المثال، إذا كان مسار ملف زوج المفاتيح (keypair) في نظام الملفات هو `/home/solana/my_wallet.json`، لعرض مسار العنوان، قُم بما يلي:

```bash
solana-keygen pubke/home/solana/my_wallet.json
```

#### المحفظة الخارجية (Hardware Wallet)

إذا إخترت محفظة خارجية (hardware wallet)، إستخدم زوج المفاتيح رابط [keypair URL](../wallet-guide/hardware-wallets.md#specify-a-hardware-wallet-key)، مثل `usb://ledger?key=0`.

```bash
solana-keygen pubke://ledger?key=0
```
