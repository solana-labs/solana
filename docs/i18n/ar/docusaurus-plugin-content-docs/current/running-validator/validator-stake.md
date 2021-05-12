---
title: إثبات الحِصَّة أو التَّحْصِيص (Staking)
---

بشكل إفتراضي لن يكون للمُدقّق (validator) الخاص بك أي حِصَّة **By default your validator will have no stake.** هذا يعني أنه لن يكون مُؤهلاً ليصبح قائدًا (leader).

## مُراقبة اللَّحاق بالركب (Monitoring Catch Up)

لتفويض حِصَّة (stake)، تأكد أولاً من تشغيل المُدقّق (validator) الخاص بك ومن أنه قد أُلحق بالمجموعة (cluster). قد يستغرق الأمر بعض الوقت للحاق بالركب بعد بدأ الإشتغال أو التمهيد (boot) للمُدقّق (validator) الخاص بك. إستخدم الأمر `catchup` لمُراقبة المُدقّق (validator) الخاص بك من خلال هذه العملية:

```bash
solana catchup ~/validator-keypair.json
```

إلى أن يتم إلتحاق المُدقّق (validator) الخاص بك، لن يكون قادرًا على التصويت بنجاح ولا يُمكن تفويض الحِصَّة (stake) له.

إذا وجدت أيضًا أن فُتحة (Slot) المجموعة (cluster) تتقدم أسرع من فُتحاتك، فمن المُحتمل ألا تلحق بالركب مُطلقًا. يُشير هذا عادةً إلى نوع من مُشكلة الشبكات بين المُدقّق (validator) وبقية المجموعة (cluster).

## إنشاء زوج مفاتيح الحِصَّة (Create Stake Keypair)

إذا لم تكن قد قُمت بذلك بالفعل، فقُم بإنشاء زوج مفاتيح إثبات حِصَّة أو تَّحْصِيص (staking keypair). إذا أكملت هذه الخطوة، يجب أن ترى "Validator-stock-keypair.json" في دليل وقت تشغيل Solana.

```bash
solana-keygen new -o ~/validator-stake-keypair.json
```

## تفويض الحِصَّة (Delegate Stake)

الآن قُم بتفويض عدد 1 SOL إلى المُدقّق (validator) الخاص بك عن طريق إنشاء حساب الحِصَّة (stake account) الخاص بك أولاً:

```bash
solana create-stake-account ~/validator-stake-keypair.json 1
```

ثم تفويض تلك الحِصَّة (stake) إلى جهة المُدقّق (validator) الخاص بك:

```bash
solana delegate-stake ~/validator-stake-keypair.json ~/vote-account-keypair.json
```

> لا تُفوض رموز SOL المُتبقية، لأن المُدقّق (validator) الخاص بك سيستخدم هذه الرموز للتصويت.

يُمكن إعادة تفويض الحِصص (stakes) إلى عُقدة (node) أخرى في أي وقت بإستخدام نفس الأمر البرمجي، ولكن يُسمح بإعادة تفويض واحدة فقط لكل فترة (epoch):

```bash
solana delegate-stake ~/validator-stake-keypair.json ~/some-other-vote-account-keypair.json
```

بإفتراض أن العُقدة (node) تقوم بالتصويت، فأنت الآن تعمل وتعمل وتحصل على مُكافآت المُدقّق (validator). يتم دفع المكافآت تلقائيًا على حدود الفترة (epoch).

يتم تقسيم المُكافآت المُكتسبة بين حساب حِصَّتك (stake) وحساب التصويت وفقًا لسعر العمولة المُحدد في حساب التصويت. لا يُمكن ربح المُكافآت إلا أثناء تشغيل المُدقّق (validator). علاوة على ذلك، بمجرد التعطيل، يُصبح المُدقّق (validator) جزءًا مُهمًا من الشبكة. من أجل إزالة المُدقّق (validator) بأمان من الشبكة، قُم أولاً بإلغاء تنشيط حِصَّته (stake).

في نهاية كل فُتحة (Slot)، من المُتوقع أن يُرسل المُدقّق (validator) مُعاملة تصويت. يتم دفع تكاليف مُعاملات التصويت هذه عن طريق الـ lamports من حساب هوية المُدقّق (validator).

هذه مُعاملة عادية لذا سيتم تطبيق رسوم المُعاملة القياسية. يتم تحديد نطاق رسوم المُعاملة بواسطة كتلة مرحلة التكوين (genesis block). سوف تتقلب الرسوم الفعلية بناءً على حمل المُعاملة. يُمكنك تحديد الرسوم الحالية عبر الحصول على تجزئة الكُتلة الحديثة [RPC API “getRecentBlockhash”](developing/clients/jsonrpc-api.md#getrecentblockhash) قبل إرسال المُعاملة.

تعرف على المزيد حول رسوم المُعاملات [transaction fees here](../implemented-proposals/transaction-fees.md).

## فترة إحماء حِصَّة المُدقّق (Validator Stake Warm-up)

لمُكافحة الهجمات المُختلفة على الإجماع (consensus)، تخضع تفويضات الحِصَّة (stake) الجديدة لفترة إحماء [warm-up](/staking/stake-accounts#delegation-warmup-and-cooldown).

مُراقبة حِصَّة (stake) المُدقّق (validator) أثناء فترة الإحماء (warmup) من خلال:

- عرض حساب التصويت الخاص بك: `solana vote-account ~/vote-account-keypair.json` يعرض هذا الوضع الحالي لجميع الأصوات التي أرسلها المُدقّق (validator) إلى الشبكة.
- عرض حساب حِصَّتك (stake) وتفضيل التفويض وتفاصيل حِصَّتك:`solana stake-account ~/validator-stake-keypair.json`
- يعرض `solana validators` الحِصَّة (stake) النشطة الحالية لجميع المُدقّقين (validators)، بما في ذلك حِصَّتك حِصَّتك
- `solana stake-history` shows the history of stake warming up and cooling down over recent epochs
- قُم بالبحث عن رسائل السجل في المُدقّق (validator) الذي يُشير إلى الفُتحة القائد (leader slot) التالية: `[2019-09-27T20:16:00.319721164Z INFO solana_core::replay_stage] <VALIDATOR_IDENTITY_PUBKEY> voted and reset PoH at tick height ####. الفُتحة القائد (leader slot) التالية هي #### `
- بمجرد أن يتم تسخين حِصَّتك (stake)، سترى رصيد حِصَّة (stake) مُدرجًا للمُدقّق (validator) الخاص بك بتشغيل `solana validators`

## مُراقبة المُدقّق المُحَصِّص الخاص بك (Monitor Your Staked Validator)

تأكد من أن المُدقّق (validator) الخاص بك يُصبح قائد [leader](../terminology.md#leader)

- بعد ضبط المُدقّق (validator) الخاص بك، إستخدم الأمر `solana balance` لمُراقبة الأرباح حيث يتم إختيار المُدقّق (validator) الخاص بك كقائد (leader) ويجمع رسوم المُعاملات
- تُقدم العُقد (nodes) في Solana عددًا من الطُرق JSON-RPC المُفيدة لعرض معلومات حول الشبكة ومُشاركة المُدقّق (validator). قُم بتقديم طلب بإستخدام curl \ (أو عميل http آخر من إختيارك \) ، مع تحديد الطريقة المطلوبة في البيانات بتنسيق JSON-RPC. على سبيل المثال:

```bash
  // Request
  curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getEpochInfo"}' http://localhost:8899

  // Result
  {"jsonrpc":"2.0","result":{"epoch":3,"slotIndex":126,"slotsInEpoch":256},"id":1}
```

طرق JSON-RPC المُساعدة:

- الأمر البرمجي " الحصول على معلومات الفترة " `getEpochInfo` [An epoch](../terminology.md#epoch) هو الوقت، أي الفُتحات [slots](../terminology.md#slot)، حيث يكون جدول القائد [leader schedule](../terminology.md#leader-schedule) صالحًا. سيُخبرك هذا ما هي الفترة (epoch) الحالية ومدى وجود المجموعة (cluster) فيها.
- سيُخبرك الأمر البرمجي " الحصول على حِسابات التصويت "`getVoteAccounts` هذا بمقدار الحِصَّة (stake) النَشِطة التي يمتلكها المُدقّق (validator) حاليًا. يتم تنشيط نسبة مئوية % من حِصَّة (stake) المُدقّق (validator) على حدود الفترة (epoch). يُمكنك معرفة المزيد حول إثبات الحِصَّة أو التَّحْصِيص (staking) على Solana هنا [here](../cluster/stake-delegation-and-rewards.md).
- الأمر البرمجي " الحصول على جدول القائد " `getLeaderSchedule` في أي لحظة، تتوقع الشبكة أن يقوم مُدقّق (validator) واحد فقط بإنتاج مُدخلات دفتر الأستاذ (ledger entries). المُدقّق المُحدد حاليًا لإنتاج مُدخلات دفتر الأستاذ [validator currently selected to produce ledger entries](../cluster/leader-rotation.md#leader-rotation) يُسمى "القائد". سيُعيد هذا الجدول الزمني الكامل للقائد \ (على أساس فُتحة تلو الأخرى \) للحِصَّة (stake) النشطة حاليًا، وسيظهر مفتاح الهوية العمومي (identity pubkey) مرة واحدة أو أكثر هنا.

## إلغاء تنشيط الحِصَّة (Deactivating Stake)

قبل فصل المُدقّق (validator) الخاص بك عن المجموعة (cluster)، يجب إلغاء تنشيط الحِصَّة (stake) التي تم تفويضها مسبقًا عن طريق تشغيل:

```bash
solana deactivate-stake ~/validator-stake-keypair.json
```

لم يتم إلغاء تنشيط الحِصَّة (stake) فورا وبدلا من ذلك تبرد (cools down) بطريقة مُماثلة كما الحِصَّة (stake) في فترة الإحماء (warmup). يجب أن يظل المُدقّق (validators) الخاصة بك متصلا بالمجموعة (cluster) أثناء فترة تبريد (cooldown) الحِصَّة (stake). أثناء فترة التبريد (cooldown)، سوف تستمر حِصَّتك (stake) في كسب المكافآت. فقط بعد فترة تبريد (cooldown) الحِصَّة (stake)، يكون من الآمن إيقاف تشغيل المُدقّق (validator) الخاص بك أو سحبه من الشبكة. قد يستغرق فترة التبريد (cooldown) عدة فترات (epochs) حتى تكتمل، إعتمادًا على الحِصَّة (stake) النشطة وحجم حِصَّتك (stake).

لاحظ أنه لا يجوز إستخدام حساب الحِصَّة (stake account) إلا مرة واحدة، لذلك بعد التعطيل، إستخدم الأمر cli's `withdraw-stake` لإستعادة الـ lamports المُكدس سابقًا.
