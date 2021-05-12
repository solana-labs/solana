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

وهو عبارة عن سلسلة المفاتيح العمومية [public keys](terminology.md#public-key) لمدقّق ما [validator](terminology.md#validator). وتستخدم المجموعة جدول القائد لتحديد القائد [leader](terminology.md#leader) من بين المدققين في أي لحظة من الزمن.

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

## الفُتحة (Slot)

هي تلك الفترة من الزمن والتي يستطيع القائد [leader](terminology.md#leader) أن يستوعب فيها المعاملات ويقوم بإنتاج كتلة [block](terminology.md#block).

## العقد الذكي (smart contract)

وهو عبارة عن مجموعة من القيود التي بمجرد حصولها تقوم بإرسال إشارة إلى برنامج ما أن بعض تحديثات حساب محدد مسبقًا قد تم السماح بها.

## عملة sol

هي عبارة عن رموز أصلية [native token](terminology.md#native-token) يمكن تتبعها بواسطة مجموعة [cluster](terminology.md#cluster) يمكن لشركة Solana التعرف عليها.

## الحِصَّة (stake)

الرموز التي يتم أخذها إلى المجموعة [cluster](terminology.md#cluster) إذا تم إثبات وجود سلوك خبيث لمدقق ما [validator](terminology.md#validator).

## الأغلبية العُظمى (supermajority)

الغالبية العظمى من مجموعة [cluster](terminology.md#cluster) هي ثلثا المجموعة.

## مُتغير النظام (sysvar)

وهو عبارة عن حساب [account](terminology.md#account) مغلف يتم توفيره بواسطة وقت التشغيل للسماح للبرامج بالوصول إلى حالة الشبكة مثل الارتفاع الحالي للعلامة، والمكافآت، والنقاط [points](terminology.md#point)، والقيم، إلخ.

## العميل الرقيق (thin client)

وهو العميل [client](terminology.md#client) الذي يثق بأنه يتواصل مع مجموعة [cluster](terminology.md#cluster) صالحة.

## العلامة (الـ tick)

هي مدخلة [entry](terminology.md#entry) في دفتر الأستاذ والتي تقوم بتقدير مدة ساعة الحائط.

## إرتفاع العلامة (tick height)

وهي العلامة [tick](terminology.md#tick) النونية في دفتر الأستاذ [ledger](terminology.md#ledger).

## الرمز (token)

هو عنصر نادر قابل للاستبدال في مجموعة الرموز.

## المُعاملة في الثانية الواحدة (tps)

عدد المعاملات [Transactions](terminology.md#transaction) المنجزة في الثانية الواحدة.

## المُعاملة (transaction)

وهي عبارة عن واحدة أو أكثر من التعليمات [instructions](terminology.md#instruction) الموقعة بواسطة العميل [client](terminology.md#client) باستخدام واحد أو أكثر من أزواج المفاتيح [keypairs](terminology.md#keypair) ويتم تنفيذها بشكل تلقائي باحتمالين اثنين فقط: النجاح أو الفشل.

## معرف المُعاملة (transaction id)

وهو عبارة عن أول توقيع [signature](terminology.md#signature) في المعاملة [transaction](terminology.md#transaction) والذي يمكن استخدامه للتعرف بشكل فريد على المعاملة في كافة دفتر الأستاذ [ledger](terminology.md#ledger).

## تأكيدات المُعاملة (transaction confirmations)

هو عبارة عن عدد الكتل المؤكدة [confirmed blocks](terminology.md#confirmed-block) منذ قبول المعاملة في دفتر الأستاذ [ledger](terminology.md#ledger). ويتم الانتهاء من المعاملة عندما تصبح كتلتها جذرًا [root](terminology.md#root).

## مُدخلة المُعاملات (transactions entry)

عبارة عن مجموعة من المعاملات [transactions](terminology.md#transaction) التي يمكن تنفيذها بالتوازي.

## المُدقّق (validator)

هو عضو له كامل الصلاحية في مجموعة [cluster](terminology.md#cluster) ويكون مسؤولًا عن تدقيق دفتر الأستاذ [ledger](terminology.md#ledger) وإنتاج كتل [blocks](terminology.md#block) جديدة.

## دالّة التأخير التي يُمكن التَحَقُّق منها (VDF)

انظر [verifiable delay function](terminology.md#verifiable-delay-function).

## دالّة التأخير التي يُمكن التَحَقُّق منها (Verifiable Delay Function)

وهي دالة تستغرق وقتًا محددًا لتنفيذها وتنتج دليلًا على تشغيلها، والذي يمكن التحقق منه فيما بعد في وقت أقل مما استغرقته في إنتاجها.

## التصويت (vote)

انظر تصويت دفتر الأستاذ [ledger vote](terminology.md#ledger-vote).

## رصيد التصويت (vote credit)

هو حصيلة مكافأة المدققين [validators](terminology.md#validator). يتم مكافأة المدقق بائتمان التصويت في حساب التصويت الخاص به عندما يصبح المدقق جذرًا [root](terminology.md#root).

## المحفظة (wallet)

هي عبارة عن مجموعة من أزواج المفاتيح [keypairs](terminology.md#keypair).

## فترة الإحماء (warmup period)

عبارة عن عدد من الفترات [epochs](terminology.md#epoch) بعد تفويض حصة [stake](terminology.md#stake) حتى تصبح فعالة بشكل تدريجي. وخلال هذه الفترة، تعتبر الحصة "منشطة". للمزيد من المعلومات حول الإحماء والتبريد: [warmup and cooldown](cluster/stake-delegation-and-rewards.md#stake-warmup-cooldown-withdrawal)
