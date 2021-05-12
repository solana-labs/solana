---
title: تفويض الحِصَّة (Delegate Stake)
---

بعد أن تتلقى عُملة [received SOL](transfer-tokens.md)، قد تُفكر في إستخدامها عن طريق تفويض الحِصَّة _stake_ للمُدقّق (validator). إثبات الحِصَّة أو التَّحْصِيص (stake) هو ما نعتبره رموز في حِساب إثبات الحِصَّة أو التَّحْصِيص _stake account_. تقدر Solana التصويت على أساس كمية الرموز التي يتم إثبات حصتها المُفَوَّضَة لهم، ما يعطي هؤلاء المُدقّقين (validators) تأثيرا أكبر في تحديد الكتلة (block) الصالحة التالية من المُعاملات في شبكة البلوكشاين. تقوم Solana بعد ذلك بتوليد عملات SOL جديدة بشكل دوري لمُكافأة المحصِّصين (stakers) والمُدقّقين (validators). تكسب المُزيد من المكافآت كلما زادت الحِصَّة (stake) التي تُفَوِّضُها.

## إنشاء حِساب إثبات الحِصَّة أو التَّحْصِيص (Stake Account)

لتفويض الحِصَّة (stake)، سوف تحتاج إلى تحويل بعض العملات إلى حِساب إثبات الحِصَّة أو التَّحْصِيص (Stake account). لإنشاء حساب، ستحتاج إلى زوج مفاتيح (keypair). سيتم إستخدام المفتاح العمومي (Pubkey) الخاص به كعنوان حِساب إثبات الحِصَّة أو التَّحْصِيص [stake account address](../staking/stake-accounts.md#account-address). لا حاجة إلى كلمة مرور أو تشفير هنا؛ سيتم تجاهل زوج المفاتيح (keypair) هذا مُباشرة بعد إنشاء حِساب إثبات الحِصَّة أو التَّحْصِيص (Stake account).

```bash
solana-keygen new --no-passphrase -o stake-account.json
```

سيحتوي المُخرج (output) على المفتاح العمومي (public key) بعد النص `pubkey:`.

```text
pubkey: GKvqsuNcnwWqPzzuhLmGi4rzzh55FhJtGizkhHaEJqiV
```

قم بنسخ المفتاح العمومي (public key) وخزنه في مكان آمن للحفاظ عليه. ستحتاج إليه في أي وقت تُريد فيه تنفيذ إجراء ما على حِساب إثبات الحِصَّة أو التَّحْصِيص (Stake account) الذي تُنشئه بعد ذلك.

أنشأ الآن حِساب إثبات الحِصَّة أو التَّحْصِيص (Stake account):

```bash
solana create-stake-account --from <KEYPAIR> stake-account.json <AMOUNT> \
    --stake-authority <KEYPAIR> --withdraw-authority <KEYPAIR> \
    --fee-payer <KEYPAIR>
```

`<AMOUNT>` يتم نقل العملات من الحساب في "من" `<KEYPAIR>` إلى حِساب إثبات حِصَّة أو تحْصِيص (Stake account) جديد في المفتاح العمومي (public key) لـ stake-account.json.

يُمكن الآن تجاهل ملف stake-account.json. للسماح بإتخاذ إجراءات إضافية، سوف تستخدم سُلطة التَّحْصِيص `--stake-authority` أو زوج مفاتيح (keypair) سُلطة سحب الرصيد `--withdraw-authority`، وليس stake-account.json.

عرض حِساب إثبات الحِصَّة أو التَّحْصِيص (Stake account) الجديد مع أمر `solana stake-account`:

```bash
حساب-إثبات الحصة الخاص بSolana <STAKE_ACCOUNT_ADDRESS>
```

سيبدو المُخرَج (output) مُماثلا لهذا:

```text
مجموع الحصة: 5000 SOL
إثبات الحصة لم يتم تفويضه
سلطة إثبات الحصة: EXU95vqs93yPeCeAU7mPPu6HbRUmTFPEGug9oCdvQ5F
سلطة السحب: EXU95vqs93yPeCeAU7mPPu6HbRUmTFPEGug9oCdvQ5F
```

### تعيين الحِصَّة (Set Stake) وسُلطة سحب الرصيد (Withdraw Authorities)

سُلطات إثبات الحِصَّة وسحب الرصيد [Stake and withdraw authorities](../staking/stake-accounts.md#understanding-account-authorities) يُمكن تعيينها عند إنشاء حساب عن طريق خيارات سُلطة التَّحْصِيص `--stake-authority` و وسحب الرصيد `--withdraw-authority`، أو بعد ذلك مع أمر الإذن بالتَّحْصِيص `solana stake-authorize`. على سبيل المثال، لتعيين سُلطة إثبات حِصَّة أو تحْصِيص (Stake authority) جديدة، قُم بتشغيل:

```bash
solana stake-authorize <STAKE_ACCOUNT_ADDRESS> \
    --stake-authority <KEYPAIR> --new-stake-authority <PUBKEY> \
    --fee-payer <KEYPAIR>
```

سوف يستخدم هذا سُلطة إثبات الحِصَّة أو التَّحْصِيص (Stake authority) الحالية `<KEYPAIR>` للإذن بسلطة إثبات حِصَّة جديدة `<PUBKEY>` على حساب إثبات الحِصَّة أو التَّحْصِيص `<STAKE_ACCOUNT_ADDRESS>`.

### خيار مُتقدم: إشتقاق عناوين حِساب إثبات الحِصَّة أو التَّحْصِيص (Derive Stake Account Addresses)

عندما تُفَوِّضُ الحِصَّة، تُفَوِّضُ جميع العملات الموجودة في الحساب إلى مُدقّق (validator) واحد فقط. للتفويض لأكثر من مُدقّق (validator)، ستحتاج إلى حِسابات إثبات حِصَّة أو تحْصِيص (Stake accounts) مُتعددة. إنشاء زوج مفاتيح (keypair) جديد لكل حساب وإدارة تلك العناوين يُمكن أن يكون عملا مُرهقا. لحسن الحظ، يُمكنك إشتقاق عناوين إثبات إثبات الحِصَّة أو التَّحْصِيص بإستخدام الخيار `--seed`:

```bash
solana create-stake-account --from <KEYPAIR> <STAKE_ACCOUNT_KEYPAIR> --seed <STRING> <AMOUNT> \
    --stake-authority <PUBKEY> --withdraw-authority <PUBKEY> --fee-payer <KEYPAIR>
```

`<STRING>` سلسلة إعتباطية تصل إلى 32 bytes، لكن ستكون عادة رقما يُقابل الحساب المُشتق. قد يكون الحساب الأول "0"، ثم "1"، وهلم جرا. المفتاح العمومي لزوج مفاتيح حِساب إثبات الحِصَّة أو التَّحْصِيص `<STAKE_ACCOUNT_KEYPAIR>` يلعب دور العنوان الأساسي. يستمد الأمر عنوانا جديدا من العنوان الأساسي والسلسلة الأولية أو الأصلية (seed string). لمعرفة عنوان إثبات الحِصَّة أو التَّحْصِيص الذي سيشمله الأمر، إستخدم الأمر `solana create-address-with -seed`:

```bash
solana create-address-with-seed --from <PUBKEY> <SEED_STRING> STAKE
```

`<PUBKEY>` هو المفتاح العمومي لزوج مفاتيح حِساب إثبات الحِصَّة أو التَّحْصِيص `<STAKE_ACCOUNT_KEYPAIR>` الذي ينتقل إلى `solana create-stake-account`.

سيُولد الأمر عنوانا مُشتقا، يمكن إستخدامه كحجة زوج مفاتيح حِساب إثبات الحِصَّة أو التَّحْصِيص `<STAKE_ACCOUNT_ADDRESS>` في عمليات إثبات الحِصَّة.

## تفويض إثبات الحِصَّة أو التَّحْصِيص (Delegate Stake)

لتفويض حِصَّتك إلى المُدقّق (validator)، ستحتاج إلى عنوان حِساب التصويت (vote account) الخاص به. إبحث عنه من خلال الإستعلام عن المجموعة لقائمة جميع المُدقّقين (validators) و حساباتهم مع أمر `solana validators`:

```bash
solana validators
```

العمود الأول من كل صف يحتوي على هوية المُدقّق (validator) و العمود الثاني هو عنوان حساب التصويت. إختر المُدقّق (validator) وإستخدم عنوان حساب التصويت الخاص به في `solana delegate-stake`:

```bash
solana delegate-stake --stake-authority <KEYPAIR> <STAKE_ACCOUNT_ADDRESS> <VOTE_ACCOUNT_ADDRESS> \
    --fee-payer <KEYPAIR>
```

سُلطة إثبات الحِصَّة أو التَّحْصِيص`<KEYPAIR>` تأذن بالعملية على الحساب مع عنوان زوج مفاتيح حِساب إثبات الحِصَّة `<STAKE_ACCOUNT_ADDRESS>`. تم تفويض الحِصَّة إلى حساب التصويت مع عنوان زوج مفاتيح حِساب إثبات الحِصَّة أو التَّحْصِيص `<VOTE_ACCOUNT_ADDRESS>`.

بعد تفويض الحِصَّة، إستخدم `solana stake-account` لمُراقبة التغييرات إلى حِساب إثبات الحِصَّة أو التَّحْصِيص (Stake account):

```bash
solana stake-account <STAKE_ACCOUNT_ADDRESS>
```

سترى في المخرجات حقلين جديدين "Delegated Stake" و "Delegated Vote Account Address". سيبدو المُخرَج (output) مماثلا لهذا:

```text
مجموع المساهمة: 5000 SOL
الإعتمادات الملحوظة: 147462
الحصة المفوضة: 4999. 9771712 SOL
عنوان حساب التصويت المفوض: CcaHc2L43ZWjwCHART3oZoJVHLAe9hzT2DJNUpzoTN1
يتم تنشيط الحصة بدءا من الحقبة: 42
سلطة الحصة: EXU95vqs93yPeCeAU7mPPu6HbRUmTFPEiGug9oCdvQ5F
سلطة الإنسحاب: EXU95vqs93yPeCAUe7mPPu6HbRUmTFPEGug9oCdvQ5F
```

## إلغاء إثبات الحِصَّة أو التَّحْصِيص (Deactivate Stake)

بمجرد تفويضك، يُمكنك إلغاء تفويضك للحِصَّة مع الأمر `solana disactive-stake`:

```bash
solana deactivate-stake --stake-authority <KEYPAIR> <STAKE_ACCOUNT_ADDRESS> \
    --fee-payer <KEYPAIR>
```

سُلطة إثبات الحِصَّة أو التَّحْصِيص `<KEYPAIR>` تأذن بالعملية على الحساب مع عنوان حِساب إثبات الحِصَّة أو التَّحْصِيص `<STAKE_ACCOUNT_ADDRESS>`.

لاحظ أن الحِصَّة تستغرق عدة فترات (epochs) حتى "تبرد" (cool down). سوف تفشل مُحاولات تفويض الحِصَّة في فترة التهدئة.

## سحب الحِصَّة (Withdraw Stake)

نقل الرموز من حساب الحِصَّة مع أمر سحب الحِصَّة `solana withdraw-stake`:

```bash
solana withdraw-stake --withdraw-authority <KEYPAIR> <STAKE_ACCOUNT_ADDRESS> <RECIPIENT_ADDRESS> <AMOUNT> \
    --fee-payer <KEYPAIR>
```

`<STAKE_ACCOUNT_ADDRESS>` هو حِساب إثبات الحِصَّة الموجود. سُلطة إثبات الحِصَّة `<KEYPAIR>` هي سُلطة سحب الرصيد، و `<AMOUNT>` هو عدد الرموز التي سيتم نقلها إلى `<RECIPIENT_ADDRESS>`.

## تقسيم الحِصَّة (Split Stake)

قد ترغب في تفويض الحِصَّة للمُدقّقين (validators) الإضافيين بينما حِصَّك الموجودة غير مُؤهلة للسحب. قد لا تكون مُؤهلة لأنها حاليا في حالة القيام بعملية إثبات الحِصَّة أو التَّحْصِيص، تبريد أو محجوزة. لنقل الرموز من حِساب إثبات الحِصَّة أو التَّحْصِيص (Stake account) الموجود إلى حساب جديد، إستخدم أمر تقسيم الحِصَّة `solana split-stake`:

```bash
solana split-stake --stake-authority <KEYPAIR> <STAKE_ACCOUNT_ADDRESS> <NEW_STAKE_ACCOUNT_KEYPAIR> <AMOUNT> \
    --fee-payer <KEYPAIR>
```

`<STAKE_ACCOUNT_ADDRESS>` هو حساب إثبات الحصة الموجود. سلطة إثبات الحصة `<KEYPAIR>` هي سلطة إثبات الحصة، `<NEW_STAKE_ACCOUNT_KEYPAIR>` هو زوج الkeypair للحساب الجديد، و `<AMOUNT>` هو عدد الرموز المراد تحويلها إلى الحساب الجديد.

لتقسيم حساب إثبات حصة إلى عنوان حساب مشتق، إستخدم خيار `--seed`. راجع عناوين حساب إثبات الحصة المشتقة [Derive Stake Account Addresses](#advanced-derive-stake-account-addresses) لمزيد من التفاصيل.
