---
title: المحفظة الورقية (Paper Wallet)
---

يصف هذا المستند كيفية إنشاء واستخدام المحفظة الورقية بواسطة أدوات واجهة سطر الأوامر (CLI) في Solana.

> نحن لا ننوي تقديم المشورة حول كيفية إنشاء أو إدارة المحافظ الورقية بشكل آمن _securely_. لذا قم بالحرص على البحث عن إجراءات الأمان من فضلك.

## نظرة عامة

تقدم Solana أداة إنشاء مفاتيح لاستخراج مفاتيح من BIP39 المطابقة لكلمات الاسترداد. جميع أوامر واجهة سطر الأوامر في Solana لتشغيل مدقق وتحصيص رموز تدعم إدخال زوج المفاتيح من خلال كلمات الاسترداد.

لمعرفة المزيد عن معيار BIP39، قم بزيارة مستودع BIPs Github من [here](https://github.com/bitcoin/bips/blob/master/bip-0039.mediawiki).

## استخدام المحفظة الورقية

يمكن تشغيل أوامر Solana بدون الحاجة مطلقًا إلى حفظ زوج مفاتيح على قرص أي جهاز. إذا كنت تعتبر تجنب كتابة مفتاح خاص على قرص أمرًا متعلقًا بالأمان، فقد أتيت إلى المكان الصحيح.

> حتى مع استخدام طريقة الإدخال الآمنة هذه، لا يزال ممكنًا أن يتم كتابة مفتاح خاص إلى قرص من خلال عمليات تبديل الذاكرة غير المشفرة. لذا، يعد من مسؤولية المستخدم أن يحمي نفسه من مثل هذا السيناريو.

## قبل البدء

- [قم بتثبيت أدوات سطر أوامر Solana](../cli/install-solana-cli-tools.md)

### تحقق من التثبيت

تحقق من أنه تم تثبيت `solana-keygen` بشكل صحيح من خلال كتابة الأمر:

```bash
solana-keygen --version
```

## إنشاء محفظة ورقية

باستخدام أداة `solana-keygen`، يمكنك إنشاء كلمات استرداد جديدة وكذلك اشتقاق زوج مفاتيح من كلمات الاسترداد وكذلك كلمة سر (اختياري). يمكن استخدام كلمات الاسترداد وكلمة السر معًا كمحفظة ورقية. وطالما أبقيت كلمات الاسترداد وكلمة السر مخزنة بشكل آمن، يمكنك استخدامهم للوصول إلى حسابك.

> لمزيد من المعلومات حول كيفية عمل كلمات الاسترداد، قم بمراجعة صفحة [Bitcoin Wiki page](https://en.bitcoin.it/wiki/Seed_phrase).

### توليد كلمات الاسترداد

يمكن إنشاء زوج مفاتيح جديد باستخدام أمر `solana-keygen الجديد` الجديد. سيقوم الأمر بإنشاء كلمات إستِرداد (seed phrase) عشوائية، ويطلب منك إختياريا إدخال عبارة المرور، ثم تعرض المفتاح العام المُشتقِّ وكلمات الإستِرداد التي تم إنشاؤها لمحفظتك الورقية (paper wallet).

بعد نسخ عبارة الكلمات الخاصة بك، يمكنك استخدام [اشتقاق المفتاح العمومي](#public-key-derivation) للتحقق من أنك لم ترتكب أي أخطاء.

```bash
solana-keygen new --no-outfile
```

> If the `--no-outfile` flag is **omitted**, the default behavior is to write the keypair to `~/.config/solana/id.json`, resulting in a [file system wallet](file-system-wallet.md)

سيظهر إخراج هذا الأمر سطر مثل هذا:

```bash
pubkey: 9ZNTfG4NyQgxy2SWjSiQoUyBPEvXT2xo7fKc5hPYYJ7b
```

القيمة المبينة بعد `pubkey:` هي _عنوان محفظتك_.

**ملاحظة:** في العمل مع المحافظ الورقية ومحافظ نظام الملفات، يتم أحيانا استخدام مصطلحي "المفتاح العام" و "عنوان المحفظة" بشكل متبادل.

> لمزيد من الأمان ، قم بزيادة عدد كلمات العبارات الأولية باستخدام الوسيطة `--word-count` الوسيطة

للحصول على تفاصيل الاستخدام الكاملة ، قم بتشغيل:

```bash
solana-keygen new --help
```

### اشتقاق المفتاح العام

يمكن اشتقاق المفاتيح العامة من عبارة أولية وعبارة مرور إذا اخترت استخدام واحدة. This is useful for using an offline-generated seed phrase to derive a valid public key. The `solana-keygen pubkey` command will walk you through how to use your seed phrase (and a passphrase if you chose to use one) as a signer with the solana command-line tools using the `ask` uri scheme.

```bash
solana-keygen pubkey prompt://
```

> لاحظ أنه من المحتمل أن تستخدم عبارات مرور مختلفة لنفس العبارة الأولية. ستؤدي كل عبارة مرور فريدة إلى زوج مفاتيح مختلف.

تستخدم أداة `solana-keygen` نفس قائمة الكلمات الإنجليزية القياسية BIP39 كما تفعل لإنشاء عبارات أولية. إذا تم إنشاء العبارة الأولية الخاصة بك باستخدام أداة أخرى تستخدم قائمة كلمات مختلفة، فلا يزال بإمكانك إستخدام `solana-keygen` ، ولكنك ستحتاج إلى تمرير `--skip-seed-phrase-validation` وسيطة والتخلي عن هذا التَحَقُّق من الصحة.

```bash
solana-keygen pubkey prompt:// --skip-seed-phrase-validation
```

After entering your seed phrase with `solana-keygen pubkey prompt://` the console will display a string of base-58 character. This is the base _wallet address_ associated with your seed phrase.

> انسخ العنوان المشتق إلى USB لسهولة الاستخدام على أجهزة الكمبيوتر المتصلة بالشبكة

> تتمثل الخطوة التالية الشائعة في [ التحقق من رصيد ](#checking-account-balance) الحساب المرتبط بمفتاح عام

للحصول على تفاصيل الاستخدام الكاملة ، قم بتشغيل:

```bash
solana-keygen pubkey --help
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

## التحقق من زوج المفاتيح

للتحقق من أنك تمتلك المفتاح الخاص لعنوان ما، قم باستخدام الأمر `solana-keygen verify`:

```bash
solana-keygen verify <PUBKEY> prompt://
```

where `<PUBKEY>` is replaced with the wallet address and the keyword `prompt://` tells the command to prompt you for the keypair's seed phrase; `key` and `full-path` query-strings accepted. Note that for security reasons, your seed phrase will not be displayed as you type. After entering your seed phrase, the command will output "Success" if the given public key matches the keypair generated from your seed phrase, and "Failed" otherwise.

## التحقق من رصيد الحساب (Checking an Account's Balance)

كل ما هو مطلوب للتحقق من رصيد الحساب هو المفتاح العام للحساب. لاسترداد المفاتيح العامة بأمان من المحفظة الورقية ، اتبع ملف [ اشتقاق المفتاح العام ](#public-key-derivation) حول تعليمات [ كمبيوتر به تداخلات هوائية ](<https://en.wikipedia.org/wiki/Air_gap_(networking)>). يمكن بعد ذلك كتابة المفاتيح العامة يدويًا أو نقلها عبر محرك أقراص USB إلى جهاز متصل بالشبكة.

بعد ذلك قم بإعداد أداة واجهة سطر أوامر `solana` من أجل الاتصال بمجموعة معينة، [connect to a particular cluster](../cli/choose-a-cluster.md):

```bash
solana config set --url <CLUSTER URL> # (i.e. https://api.mainnet-beta.solana.com)
```

أخيرًا، للتحقق من الرصيد قم بكتابة الأمر التالي:

```bash
solana balance <PUBKEY>
```

## إنشاء عناوين في المحفظة الورقية

يمكنك إنشاء أي عدد تريده من عناوين المحفظة. قم ببساطة بإعادة الخطوات في توليد كلمات الاسترداد [Seed Phrase Generation](#seed-phrase-generation) أو إنشاء مفتاح عام [Public Key Derivation](#public-key-derivation) لإنشاء عنوان جديد. يمكن أن تكون عناوين المحفظة المتعددة مفيدة إذا كنت ترغب في نقل الرموز بين حساباتك الخاصة لأغراض مختلفة.

## الدعم

قم بزيارة صفحة الدعم الخاصة بالمحفظة [Wallet Support Page](support.md) لمعرفة طرق الحصول على مساعدة.
