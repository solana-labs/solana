---
title: المُصطلحات
---

هذه المُصطلحات الآتية مُستخدمة بكثرة في جميع الوثائق.

## الحساب

ملف دائم تتم معالجته بواسطة المفتاح العام [ public key](terminology.md#public-key) وبواسطة [lamports](terminology.md#lamport) تتبعه بشكل دائم.

## تطبيق

تطبيق واجهة أمامية يتفاعل مع مجموعة Solana.

## حالة البنك

نتيجة تفسير جميع البرامج في دفتر الأستاذ عند إرتفاع علامة [tick height](terminology.md#tick-height) مُعين. ويتضمن على الأقل مجموعة جميع الحسابات [accounts](terminology.md#account) التي تحتوي على رموز [native tokens](terminology.md#native-tokens) غير صفرية.

## الكُتلة (block)

وهي مجموعة متلاصقة من المدخلات [entries](terminology.md#entry) في دفتر الأستاذ (ledger) ويغطيها صوت [vote](terminology.md#ledger-vote). ينتج القائد [leader](terminology.md#leader) بحد أقصى كتلة واحدة لكل فُتحة [slot](terminology.md#slot).

## تجزئة الكُتلة (blockhash)

هي تجزئة مقاومة الصورة الأولية [hash](terminology.md#hash) في دفتر الأستاذ [ledger](terminology.md#ledger) عند ارتفاع كتلة [block height](terminology.md#block-height) معين. وهو مأخوذ من آخر مُعرف دخول [entry id](terminology.md#entry-id) في الفُتحة

## إرتفاع الكتلة (block height)

وهو عدد الكتل [blocks](terminology.md#block) أسفل الكتلة الحالية. أول كتلة بعد كتلة مرحلة التكوين[genesis block](terminology.md#genesis-block) لها إرتفاع بقيمة واحد.

## المُدقّق التمهيدي (bootstrap validator)

هو أول مُدقّق [validator](terminology.md#validator) يقوم بإنتاج كتلة [block](terminology.md#block).

## كتلة CBC

وهي أصغر جزء مشفر من دفتر الأستاذ (ledger)، وأي قطعة مشفرة من دفتر الأستاذ (ledger) ستكون مكونة من مجموعة كتل CBC. `ledger_segment_size / cbc_block_size` ليكون دقيقًا.

## العميل (Client)

A [node](terminology.md#node) والتي تستخدم المجموعة [cluster](terminology.md#cluster).

## المجموعة (cluster)

وهي عبارة عن مجموعة من المدقّقين [validators](terminology.md#validator) الذين يقومون بالحفاظ على دفتر أستاذ [ledger](terminology.md#ledger) واحد.

## زمن التأكيد

الزمن بحسب ساعة الحائط بين إنشاء قائد [leader](terminology.md#leader) علامة دخول [tick entry](terminology.md#tick) وإنشائه كتلة مؤكدة [confirmed block](terminology.md#confirmed-block).

## كتلة مؤكدة

هي كتلة [block](terminology.md#block) قد حصلت على غالبية [supermajority](terminology.md#supermajority) في أصوات دفتر الأستاذ [ledger votes](terminology.md#ledger-vote) بتفسير لدفتر الأستاذ يتوافق مع الذي لدى القائد.

## لوحة التحكم (control plane)

وهي عبارة عن شبكة القيل والقال والتي تربط جميع العقد [nodes](terminology.md#node) في مجموعة ما [cluster](terminology.md#cluster).

## فترة التبريد (cooldown period)

عبارة عن عدد من الفترات [epochs](terminology.md#epoch) بعد تعطيل حصة [stake](terminology.md#stake) حتى تصبح متاحة للسحب تدريجيًا. خلال تلك الفترة، يعتبر التحصيص "غير منشِّط". للمزيد من المعلومات حول الإحماء والتبريد: [warmup and cooldown](implemented-proposals/staking-rewards.md#stake-warmup-cooldown-withdrawal)

## الرصيد (credit)

راجع إئتمان التصويت [vote credit](terminology.md#vote-credit).

## لوحة البيانات (data plane)

وهي عبارة عن شبكة متعددة البث والتي تستخدم لتدقيق المدخلات [entries](terminology.md#entry) بكفاءة والحصول على إجماع.

## الطائرة الآلية (drone)

وهي عبارة عن خدمة خارج السلسلة والتي تقوم بدور القيّم على المفتاخ الخاص للمستخدم. وعادة ما تقوم بتدقيق المعاملات وتوقيعها.

## المُدخلة (entry)

وتكون المدخلة في دفتر الإستاذ [ledger](terminology.md#ledger) إما عبارة عن علامة [tick](terminology.md#tick) أو مدخلة معاملات [transactions entry](terminology.md#transactions-entry).

## مُعرف المُدخلة (entry id)

هي تجزئة مقاومة الصورة الأولية [hash](terminology.md#hash) على المحتويات النهائية لمدخلة ما، والتي تعمل كمعرف فريد عالمي ل [entry's](terminology.md#entry). وتعتبر التجزئة بمثابة دليل على:

- أن المُدخلة تم توليدها بعد فترة من الزمن
- أن المعاملات [transactions](terminology.md#transaction) المحددة هي تلك المدرجة في المدخلة
- موقع المدخلة بالنسبة لغيرها من المدخلات في دفتر الأستاذ [ledger](terminology.md#ledger)

راجع إثبات التاريخ [Proof of History](terminology.md#proof-of-history).

## الفترة (epoch)

وهي عبارة عن المدة، أو عدد الفُتحات [slots](terminology.md#slot) على سبيل المثال، والتي يكون جدول القائد [leader schedule](terminology.md#leader-schedule) عندها صالحًا.

## حساب الرسوم

يتعبر حساب الرسوم في المعاملات بأنه الذي يدفع تكلفة إدخال المعاملة في دفتر الأستاذ (ledger). وهذا هو الحساب الأول في المعاملة. يجب أن يتم الإعلان بأن هذا الحساب قابل للقراءة والكتابة (يمكن التعديل عليه) في المعاملة لأن الدفع للمعاملة يؤخذ من ميزانية الحساب.

## وقت إثبات المُعاملة (Finality)

عندما تتشارك العقد التي تمثل ثلثي الحصة [stake](terminology.md#stake) نفس الجذر [root](terminology.md#root).

## الإنقسام أو الشوكة (fork)

وهو عبارة عن [ledger](terminology.md#ledger) مأخوذ من مدخلات مشتركة ولكنه يتباعد بعد ذلك.

## كتلة مرحلة التكوين (genesis block)

وهي عبارة عن أول كتلة [block](terminology.md#block) في السلسلة.

## تحضير إعدادات مرحلة التكوين

وهو عبارة عن ملف التكوين الذي يقوم بتجهيز دفتر الأستاذ [ledger](terminology.md#ledger) من أجل كتلة مرحلة التكوين [genesis block](terminology.md#genesis-block).

## التجزئة (hash)

وهي عبارة عن بصمة رقمية لسلسلة من البايتات.

## التَضَخُّم (inflation)

وهو عبارة عن زيادة في معروض الرموز مع الزمن المستغرق من أجل تمويل مكافآت التدقيق وتمويل التطوير المستمر لـ Solana.

## التعليمات (Instructions)

وهي عبارة عن أصغر وحدة في برنامج ما [program](terminology.md#program) والتي يمكن للعميل [client](terminology.md#client) أن يدرجها في أي معاملة [transaction](terminology.md#transaction).

## زوج المفاتيح (keypair)

ويتكون من مفتاح عام [public key](terminology.md#public-key) ومفتاح خاص [private key](terminology.md#private-key) مقابل له.

## لامبورت (lamport)

هو عبارة عن رمز أصلي [native token](terminology.md#native-token) كسري يحمل قيمة 0.000000001 [sol](terminology.md#sol).

## القائد (leader)

هو الدور الذي يلعبه المدقق [validator](terminology.md#validator) عندما يسمح للمدخلات [entries](terminology.md#entry) إلى دفتر الأستاذ [ledger](terminology.md#ledger).

## جدول القائد (leader schedule)

A sequence of [validator](terminology.md#validator) [public keys](terminology.md#public-key) mapped to [slots](terminology.md#slot). وتستخدم المجموعة جدول القائد لتحديد القائد [leader](terminology.md#leader) من بين المدققين في أي لحظة من الزمن.

## دفتر الأستاذ (ledger)

وهو عبارة عن قائمة من المدخلات [entries](terminology.md#entry) التي تحتوي على معاملات [transactions](terminology.md#transaction) الموقعة بواسطة عملاء [clients](terminology.md#client). من الناحية النظرية، يمكن تتبعه إلى كتلة مرحلة التكوين [genesis block](terminology.md#genesis-block)، إلا أن دفتر الأستاذ الفعلي الخاص بالمدققين [validators](terminology.md#validator) قد يحتوي فقط على كتل [blocks](terminology.md#block) للحفاظ على استخدام ذاكرة التخزين لأن القديم منها غير لازم لتدقيق الكتل المستقبلية بحسب التصميم.

## تصويت دفتر الأستاذ

وهو عبارة عن تجزئة [hash](terminology.md#hash) حالة المدقّق [validator's state](terminology.md#bank-state) عند ارتفاع علامة معين [tick height](terminology.md#tick-height). ويشمل التحقق من تأكيد المدقق [validator's](terminology.md#validator) بشأن استلامه كتلة معينة [block](terminology.md#block)، وكذلك الالتزام بعدم التصويت لكتلة [block](terminology.md#block) متعارضة \ (مثلًا: شوكة [fork](terminology.md#fork)\) لمدة محددة من الوقت، فترة الإغلاق [lockout](terminology.md#lockout).

## العميل الخفيف (light client)

هو ذلك العميل [client](terminology.md#client) الذي يمكنه أن يؤكد أنه يشير إلى مجموعة [cluster](terminology.md#cluster) صالحة. ويقوم بالتحقق من دفتر الأستاذ أكثر من العميل الرقيق [thin client](terminology.md#thin-client) وأقل من المدقّق [validator](terminology.md#validator).

## المُحمِّل (Loader)

وهو عبارة عن برنامج [program](terminology.md#program) لديه القدرة على تفسير الترميز الثنائي للبرامج الأخرى على السلسلة.

## فترة القِفْل (lockout)

وهي الفترة التي يكون فيها المدقّقُ [validator](terminology.md#validator) غير قادر على التصويت [vote](terminology.md#ledger-vote) على شوكة أو انقسام [fork](terminology.md#fork) آخر.

## الرمز الأصلي (native token)

هو رمز [token](terminology.md#token) يستخدم لتتبع العمل الذي قامت به العقد [nodes](terminology.md#node) في مجموعة ما.

## العُقدة (node)

وهو عبارة عن جهاز حاسوب يعمل ضمن مجموعة [cluster](terminology.md#cluster).

## عدد العُقدة (node count)

وهو عدد المدققين [validators](terminology.md#validator) المشاركين في مجموعة [cluster](terminology.md#cluster).

## إثبات التاريخ (PoH)

انظر [Proof of History](terminology.md#proof-of-history).

## النقطة (point)

وهي رصيد ذا وزن [credit](terminology.md#credit) في نظام المكافآت. في نظام مكافآت المدقق [validator](terminology.md#validator) [rewards regime](cluster/stake-delegation-and-rewards.md)، يكون عدد النقاط التي تستحقها حصة ما [stake](terminology.md#stake) خلال فترة سداد المدفوعات هو ناتج رصيد التصويت [vote credits](terminology.md#vote-credit) المكتسب وعدد الـ lamports الخاضعة للتحصيص.

## المفتاح الخاص (private key)

وهو المفتاح الخاص من زوج المفاتيح [keypair](terminology.md#keypair).

## البرنامج (program)

هو عبارة عن شيفرة تفسر التعليمات [instructions](terminology.md#instruction).

## مُعرف البرنامج (program id)

وهو المفتاح العام الخاص بحساب [account](terminology.md#account) يحتوي على برنامج [program](terminology.md#program).

## إثبات التاريخ

وهو عبارة عن حصة الإثباتات والتي يثبت كل واحد منها أن بعض البيانات قد وجدت قبل إنشاء الإثبات وأن مدة محددة من الوقت قد مرت قبل الإثبات الأخير. تمامًا مثل دالة التأخير القابلة للتحقق [VDF](terminology.md#verifiable-delay-function)، يمكن أن يتم توثيق دليل إثبات التاريخ في وقت أقل من الوقت المستهلك في إنشائه.

## المفتاح العمومي (public key)

وهو المفتاح العام من زوج المفاتيح [keypair](terminology.md#keypair).

## الجذر (root)

هو عبارة عن كتلة [block](terminology.md#block) أو فُتحة [slot](terminology.md#slot) وصلت أقصى إغلاق [lockout](terminology.md#lockout) على مدقق [validator](terminology.md#validator). ويعد الجذر أعلى كتلة والتي تعتبر أصل جميع الانقسامات على مدقق ما. وتعتبر جميع كتل جذر معين جذرًا بالتعدي. والكتل التي لا تعد أصلًا ولا فرعًا للجذر يتم استثناؤها ولا تؤخذ بعين الاعتبار في الإجماع، ومن الممكن أن يتم تجاهلها.

## وقت التشغيل (runtime)

وهو المكون الذي يكون فيه المدقّقُ [validator](terminology.md#validator) مسؤولًا عن تشغيل برنامج معين [program](terminology.md#program).

## القِطَعة (shred)

هي عبارة عن جزء من الكتلة [block](terminology.md#block)؛ وهي أصغر وحدة مرسلة بين المدقّقين [validators](terminology.md#validator).

## التوقيع (signature)

وهو توقيع من نوع 64-byte ed25519 لR (32-bytes) و S (32-bytes). بشرط أن تكون R عبارة عن نقطة إدوارد ليست ذات ترتيب صغير، و S مدرج على مدى 0 <= S < L. هذا الشرط يضمن عدم حصول تحول في التوقيع. يجب أن يكون لكل معاملة توقيع واحد على الأقل لحساب الرسوم [fee account](terminology#fee-account). وبالتالي، يمكن لأول توقيع في المعاملة أن يتم التعامل معه على أنه معرف المعاملة [transacton id](terminology.md#transaction-id)

## skipped slot

A past [slot](terminology.md#slot) that did not produce a [block](terminology.md#block), because the leader was offline or the [fork](terminology.md#fork) containing the slot was abandoned for a better alternative by cluster consensus. A skipped slot will not appear as an ancestor for blocks at subsequent slots, nor increment the [block height](terminology#block-height), nor expire the oldest `recent_blockhash`.

Whether a slot has been skipped can only be determined when it becomes older than the latest [rooted](terminology.md#root) (thus not-skipped) slot.

## slot

The period of time for which each [leader](terminology.md#leader) ingests transactions and produces a [block](terminology.md#block).

Collectively, slots create a logical clock. Slots are ordered sequentially and non-overlapping, comprising roughly equal real-world time as per [PoH](terminology.md#proof-of-history).

## smart contract

A set of constraints that once satisfied, signal to a program that some predefined account updates are permitted.

## sol

The [native token](terminology.md#native-token) tracked by a [cluster](terminology.md#cluster) recognized by the company Solana.

## stake

Tokens forfeit to the [cluster](terminology.md#cluster) if malicious [validator](terminology.md#validator) behavior can be proven.

## supermajority

2/3 of a [cluster](terminology.md#cluster).

## sysvar

A synthetic [account](terminology.md#account) provided by the runtime to allow programs to access network state such as current tick height, rewards [points](terminology.md#point) values, etc.

## thin client

A type of [client](terminology.md#client) that trusts it is communicating with a valid [cluster](terminology.md#cluster).

## tick

A ledger [entry](terminology.md#entry) that estimates wallclock duration.

## tick height

The Nth [tick](terminology.md#tick) in the [ledger](terminology.md#ledger).

## token

A scarce, fungible member of a set of tokens.

## tps

[Transactions](terminology.md#transaction) per second.

## transaction

One or more [instructions](terminology.md#instruction) signed by the [client](terminology.md#client) using one or more [keypairs](terminology.md#keypair) and executed atomically with only two possible outcomes: success or failure.

## transaction id

The first [signature](terminology.md#signature) in a [transaction](terminology.md#transaction), which can be used to uniquely identify the transaction across the complete [ledger](terminology.md#ledger).

## transaction confirmations

The number of [confirmed blocks](terminology.md#confirmed-block) since the transaction was accepted onto the [ledger](terminology.md#ledger). A transaction is finalized when its block becomes a [root](terminology.md#root).

## transactions entry

A set of [transactions](terminology.md#transaction) that may be executed in parallel.

## validator

A full participant in the [cluster](terminology.md#cluster) responsible for validating the [ledger](terminology.md#ledger) and producing new [blocks](terminology.md#block).

## VDF

See [verifiable delay function](terminology.md#verifiable-delay-function).

## verifiable delay function

A function that takes a fixed amount of time to execute that produces a proof that it ran, which can then be verified in less time than it took to produce.

## vote

See [ledger vote](terminology.md#ledger-vote).

## vote credit

A reward tally for [validators](terminology.md#validator). A vote credit is awarded to a validator in its vote account when the validator reaches a [root](terminology.md#root).

## wallet

A collection of [keypairs](terminology.md#keypair).

## warmup period

Some number of [epochs](terminology.md#epoch) after [stake](terminology.md#stake) has been delegated while it progressively becomes effective. During this period, the stake is considered to be "activating". More info about: [warmup and cooldown](cluster/stake-delegation-and-rewards.md#stake-warmup-cooldown-withdrawal)
