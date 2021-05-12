---
title: "الحسابات (Accounts)"
---

## تخزين الحالة بين المُعاملات (Storing State between Transactions)

إذا كان البرنامج يحتاج إلى تخزين الحالة بين المُعاملات، فإنه يفعل ذلك باستخدام _accounts_. الحسابات مُماثلة للملفات الموجودة في نظم التشغيل مثل لينوكس (Linux). مثل الملف، قد يحتفظ الحساب ببيانات عشوائية وتستمر هذه البيانات إلى ما بعد عمر البرنامج. كذلك مثل الملف أيضًا، يتضمن الحساب بيانات وصفية تُخبر وقت التشغيل بمن يُسمح له بالوصول إلى البيانات وكيفية ذلك.

على خلاف الملف، يتضمن الحساب بيانات التعريف لمدى عمر الملف. يتم التعبير عن هذا العمر في "الرموز" ، وهي عدد من الرموز الكسرية الأصلية، تُسمى _ lamports _. يتم الإحتفاظ بالحسابات في ذاكرة المُدقّق (validator) ودفع الإيجار ["rent"](#rent) للبقاء هناك. كل مُدقّق (validator) يقوم دوريا بمسح جميع الحسابات وجمع الإيجار. يتم تطهير أي حساب ينخفض إلى صفر lamports. Accounts can also be marked [rent-exempt](#rent-exemption) if they contain a sufficient number of lamports.

بنفس الطريقة التي يُستخدم بها مُستخدم Linux مسارًا للبحث عن ملف، يستخدم عميل Solana عنوان _address_ للبحث عن حساب. العنوان هو مفتاح عمومي (Pubkey) بتشفير 256-bit.

## المُوَقِّعون (signers)

قد تتضمن المُعاملات توقيعات [signatures](terminology.md#signature) رقمية تتوافق مع المفاتيح العمومية (public keys) للحسابات المُشار إليها في المُعاملة. عند وجود توقيع رقمي مطابق، فإنه يُشير إلى أن صاحب المفتاح الخاص (private key) للحساب وقّع وبالتالي "أذن" بالمُعاملة وثم يشار إلى الحساب بإسم المُوَقِّع _signer_. سواء كان الحساب مُوقِّعًا (signer) أم لا، يتم إبلاغ البرنامج كجزء من البيانات الوصفية للحساب. يُمكن للبرامج بعد ذلك إستخدام هذه المعلومات لإتخاذ قرارات السلطة.

## للقراءة فقط (Read-only)

يُمكن أن تُشير المعاملات [indicate](transactions.md#message-header-format) أن بعض الحسابات التي تُشير إليها يتم التعامل معها على أنها حسابات للقراءة فقط _read-only accounts_ من أجل تمكين مُعالجة الحساب المُوازية بين المُعاملات. يسمح وقت التشغيل بقراءة حسابات القراءة فقط بشكل مُتزامن بواسطة برامج مُتعددة. إذا حاول أحد البرامج تعديل حساب للقراءة فقط (read-only)، فسيتم رفض المُعاملة بحلول وقت التشغيل.

## قابل للتنفيذ (Executable)

إذا تم وضع علامة "قابل للتنفيذ" على الحساب في بياناته الوصفية، فإنه يُعتبر برنامجًا يُمكن تنفيذه من خلال تضمين المفتاح العمومي (public key) للحساب وتعليمات مُعرف البرنامج [program id](transactions.md#program-id). يتم وضع علامة على الحسابات كقابلة للتنفيذ أثناء عملية نشر البرنامج بنجاح بواسطة المُحمّل (Loader) الذي يمتلك الحساب. على سبيل المثال، أثناء نشر برنامج BPF، بمجرد أن يُقرر المُحمّل (Loader) أن رمز BPF الثانوي في بيانات الحساب صالح، يقوم المُحمّل بوضع علامة على حساب البرنامج بشكل دائم على أنه قابل للتنفيذ. بمجرد أن يكون قابلاً للتنفيذ، يفرض وقت التشغيل أن بيانات الحساب (البرنامج) غير قابلة للتغيير.

## الإنشاء (Creating)

لإنشاء حساب، يقوم العميل بتوليد زوج مفاتيح _keypair_ ويُسجل مفتاحه العام بإستخدام التعليمات `SystemProgram::CreateAccount` مع تخصيص حجم تخزين ثابت مُسبقًا بالـ bytes. الحد الأقصى الحالي لحجم بيانات الحساب هو 10 megabytes.

يُمكن أن يكون عنوان الحساب بأي قيمة عشوائية من 256 bit، وهناك آليات للمُستخدمين المُتقدمين لإنشاء عناوين مُشتقّة (`SystemProgram::CreateAccountWithSeed`, [`Pubkey::CreateProgramAddress`](calling-between-programs.md#program-derived-addresses)).

الحسابات التي لم يتم إنشاؤها عن طريق برنامج النظام يمكن أيضا تمريرها إلى البرامج. عندما تُشير التعليمات إلى حساب لم يتم إنشاؤه مسبقًا، فسيتم تمرير حساب مملوك لبرنامج النظام، ولا يحتوي على أي lamports، وصفر بيانات. لكن الحساب سوف يعكس سواء كان مُوقّعا (signer) على المُعاملة أم لا، وبالتالي يُمكن إستخدامه كسلطة. تنقل السلطات في هذا السياق إلى البرنامج أن صاحب المفتاح الخاص (private key) المُرتبط بالمفتاح العمومي (Pubkey) للحساب وقّع المُعاملة. قد يكون المفتاح العمومي (public key) للحساب معروفًا للبرنامج أو مُسجلًا في حساب آخر ويُشير إلى نوع من الملكية أو السلطة على أحد الأصول أو العملية التي يتحكم فيها البرنامج أو يؤديها.

## الملكية وإسنادها إلى البرامج (Ownership and Assignment to Programs)

تتم تهيئة الحساب الذي تم إنشاؤه ليكون _owned_ بواسطة برنامج مُدمج يُسمى برنامج النظام ويُسمى حساب النظام _system account_ بجدارة. يشمل الحساب بيانات تعريف "المالك". المالك هو رقم تعريف للبرنامج (program id). وقت التشغيل يمنح البرنامج الوصول للكتابة إلى الحساب إذا كان مُعرفه يطابق المالك. بالنسبة لحالة برنامج النظام، وقت التشغيل يسمح للعملاء بنقل الlamports والأهم تعيين ملكية الحساب _assign_، يعني تغيير المالك إلى مُعرف برنامج مختلف. إذا كان حساب غير مملوك للبرنامج، لا يُسمح للبرنامج إلا بقراءة البيانات الخاصة به والإيداع في الحساب.

## Verifying validity of unmodified, reference-only accounts

For security purposes, it is recommended that programs check the validity of any account it reads but does not modify.

The security model enforces that an account's data can only be modified by the account's `Owner` program. Doing so allows the program to trust that the data passed to them via accounts they own will be in a known and valid state. The runtime enforces this by rejecting any transaction containing a program that attempts to write to an account it does not own. But, there are also cases where a program may merely read an account they think they own and assume the data has only been written by themselves and thus is valid. But anyone can issues instructions to a program, and the runtime does not know that those accounts are expected to be owned by the program. Therefore a malicious user could create accounts with arbitrary data and then pass these accounts to the program in the place of a valid account. The arbitrary data could be crafted in a way that leads to unexpected or harmful program behavior.

To check an account's validity, the program should either check the account's address against a known value or check that the account is indeed owned correctly (usually owned by the program itself).

One example is when programs read a sysvar. Unless the program checks the address or owner, it's impossible to be sure whether it's a real and valid sysvar merely by successful deserialization. Accordingly, the Solana SDK [checks the sysvar's validity during deserialization](https://github.com/solana-labs/solana/blob/a95675a7ce1651f7b59443eb146b356bc4b3f374/sdk/program/src/sysvar/mod.rs#L65).

If the program always modifies the account in question, the address/owner check isn't required because modifying an unowned (could be the malicious account with the wrong owner) will be rejected by the runtime, and the containing transaction will be thrown out.

## تأجير

يتطلب الإحتفاظ بالحسابات شغالة في Solana تكلفة تخزين تسمى _rent_ لأن الكتلة (cluster) يجب أن تُحافظ بنشاط على البيانات لمعالجة أي مُعاملات مُستقبلية عليها. هذا يختلف عن الBitcoin و الEthereum، حيث لا يُؤدي تخزين الحسابات إلى أي تكاليف.

يتم خصم الإيجار من رصيد الحساب بحلول بدء التشغيل عند الوصول الأول (بما في ذلك إنشاء الحساب الأولي) في الفترة (epoch) الحالية عن طريق المُعاملات أو مرة واحدة في كل فترة إذا لم تكن هناك مُعاملات. الرسوم بمعدل ثابت حاليًا، تُقاس بالbytes وعدد الفترات (bytes-times-epochs). قد تتغير الرسوم في المستقبل.

من أجل حساب الإيجار البسيط، يتم دائمًا تحصيل الإيجار لفترة (epoch) واحدة كاملة. الإيجار غير مقسمة بالتناسب، مما يعني أنه لا توجد رسوم أو مبالغ مُستردة للفترات (epoch) الجزئية. هذا يعني أنه عند إنشاء الحساب، فإن الإيجار الأول الذي تم جمعه ليس للفترة (epoch) الجزئية الحالية، لكن جمعت مسبقا للفترة (epoch) الكاملة التالية. مجموعات الإيجار اللاحقة هي لفترات (epoch) مستقبلية أخرى. من ناحية أخرى، إذا إنخفض رصيد حساب تم تحصيل إيجاره بالفعل إلى أقل من منتصف فترة رسوم الإيجار الأخرى، فسيستمر الحساب في الوجود خلال الفترة (epoch) الحالية وسيتم تطهيره فورًا في بداية الفترة القادمة.

يُمكن إعفاء الحسابات من دفع الإيجار إذا حافظت على حد أدنى من الرصيد. يتم وصف هذا الإعفاء من الإيجار أدناه.

### حساب الإيجار (Calculation of rent)

ملاحظة: يمكن أن يتغير سعر الإيجار في المستقبل.

حتى كتابة هذا التقرير، يبلغ رسم الإيجار الثابت 19.055441478439427 lamports لكل byte-epoch على مجموعات (clusters) الشبكة التجريبية (testnet) والشبكة التجريبية الرئيسية (mainnet-beta). من المُستهدف أن تكون [epoch](terminology.md#epoch) يومين (بالنسبة إلى شبكة المطورين devnet، تبلغ رسوم الإيجار 0.3608183131797095 lamports لكل byte-epoch مع حقبة 54m36s طويلة).

هذه القيمة محسوبة على أنها الهدف 0.01 SOL في اليوم الواحد (تُطابق بالضبط إلى 3.56 SOL في كل mebibyte-year):

```text
رسوم الإيجار: 19.055441478439427 = 10_000_000 (0.01 SOL) * 365(approx. day in a year) / (1024 * 1024)(1 MiB) / (365.25/2)(epochs in 1 year)
```

يتم حساب الإيجار بدقة `f64` والنتيجة النهائية مُقتطعة إلى `u64` بالـ lamports.

حساب الإيجار يشمل بيانات تعريف الحساب (العنوان، المالك، الـ lamports، إلخ) في حجم الحساب. لذلك فإن أصغر حساب يمكن أن يكون لحسابات الإيجار هو 128 bytes.

على سبيل المثال، يتم إنشاء حساب مع التحويل الأولي لـ 10,000 من الـ lamports ولا توجد بيانات إضافية. يتم خصم الإيجار على الفور عند الإنشاء، مما يؤدي إلى رصيد قيمته 7561 lamports:

```text
الإيجار: 2,439 = 19.055441478439427 (rent rate) * 128 bytes (minimum account size) * 1 (epoch)
Account Balance: 7,561 = 10,000 (transfered lamports) - 2,439 (this account's rent fee for an epoch)
```

سيتم تخفيض رصيد الحساب إلى 5122 lamports في الفترة (epoch) التالية حتى لو كان لا يوجد نشاط:

```text
رصيد الحساب: 5,122 = 7,561 (current balance) - 2,439 (this account's rent fee for an epoch)
```

وفقًا لذلك، ستتم إزالة الحساب ذي الحجم الأدنى فورًا بعد الإنشاء إذا كانت الlamports المنقولة أقل من أو تساوي 2،439.

### الإعفاء من الإيجار (Rent exemption)

بدلاً من ذلك، يمكن جعل الحساب معفيًا تمامًا من تحصيل الإيجار عن طريق إيداع ما لا يقل عما قيمته سنتين من الإيجار. يتم التحقق من هذا في كل مرة يُخفض فيها رصيد حساب ويُخصم الإيجار فورا بمجرد أن يقل الرصيد عن الحد الأدنى للمبلغ.

حسابات البرنامج القابلة للتنفيذ مطلوبة بحلول وقت التشغيل لتكون مُعفاة من الإيجار لتجنب التطهير.

ملاحظة: إستخدم الحصول على الحد الأدنى من الرصيد للإعفاء من الإيجار [`getMinimumBalanceForRentExemption`RPC endpoint](developing/clients/jsonrpc-api.md#getminimumbalanceforrentexemption) لحساب الحد الأدنى للرصيد لحجم حساب معين. الحساب التالي هو توضيحي فقط.

على سبيل المثال، البرنامج القابل للتنفيذ بحجم 15,000 bytes يتطلب رصيد يبلغ عدد 105.290880 lamports أو (=~ 0.105 SOL) ليكون مُعفى من الإيجار:

```text
105,290,880 = 19.055441478439427 (fee rate) * (128 + 15_000)(account size including metadata) * ((365.25/2) * 2)(epochs in 2 years)
```

Rent can also be estimated via the [`solana rent` CLI subcommand](cli/usage.md#solana-rent)

```text
$ solana rent 15000
Rent per byte-year: 0.00000348 SOL
Rent per epoch: 0.000288276 SOL
Rent-exempt minimum: 0.10529088 SOL
```

Note: Rest assured that, should the storage rent rate need to be increased at some point in the future, steps will be taken to ensure that accounts that are rent-exempt before the increase will remain rent-exempt afterwards
