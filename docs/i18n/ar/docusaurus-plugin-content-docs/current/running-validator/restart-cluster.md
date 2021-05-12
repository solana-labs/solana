## إعادة تشغيل مجموعة (Restarting a cluster)

### الخطوة 1. حدد الفُتحة (cluster) التي سيتم إعادة التشغيل عندها

أعلى فُتحة (cluster) تم تأكيدها بتفاؤل هي أفضل فُتحة يُمكن البدء منها، والتي يُيمكن العثور عليها من خلال البحث عن نقطة بيانات المقاييس هذه [this](https://github.com/solana-labs/solana/blob/0264147d42d506fb888f5c4c021a998e231a3e74/core/src/optimistic_confirmation_verifier.rs#L71). وإلا إستخدم الجذر (root) الأخير.

إتصل بهذه الفُتحة `SLOT_X`

### الخطوة 2. إيقاف المُدقّق / المُدقّقين (validators)

### الخطوة 3. إختياريًا، قُم بتثبيت إصدار solana الجديد

### الخطوة 4. قم بإنشاء لقطة (snapshot) جديدة للفُتحة `SLOT_X` بإنقسام أو شوكة (fork) في الفُتحة `SLOT_X`

```bash
$ solana-ledger-tool -l ledger create-snapshot SLOT_X ledger --hard-fork SLOT_X
```

يجب أن يحتوي دليل دفتر الأستاذ (ledger) الآن على اللقطة (snapshot) الجديدة. ستقوم أداة `solana-ledger-tool create-snapshot` أيضًا بإخراج الإصدار الجديد القِطَعة (Shred), وقيمة تجزئة البنك (bank hash)، قُم بإستدعاء هذا NEW \ \_SHRED \ \_VERSION و NEW \ \_BANK \ \_HASH على التوالي.

قُم بضبط حجج المُدقّق (validator):

```bash
 --wait-for-supermajority SLOT_X
 --expected-bank-hash NEW_BANK_HASH
```

قُم بإعادة تشغيل المُدقّق (validator).

تأكد من السجل أن المُدقّق (validator) قد بدأ إشتغاله أو التمهيده (booted) وهو الآن في وضع إيقاف عند `SLOT_X`، في إنتظار الأغلبية العظمى (super majority).

### الخطوة 5. إعلان إعادة التشغيل على قناة الـ Discord:

قُم بنشر شيئ مماثل لما يلي في #announcements (قُم بضبط النص بالشكل المناسب):

> مرحبا أيها المُدقّقون (Hi @Validators,)
>
> لقد قمنا بإصدار النسخة v1.1.12 ومُستعدون لإستعادة الشبكة التجريبية مرة أخرى (We've released v1.1.12 and are ready to get testnet back up again).
>
> الخطوات: 1. قُم بتثبيت الإصدار v1.1.12: https://github.com/solana-labs/solana/releases/tag/v1.1.12 2. a. الطريقة المُفضلة، إبدأ من دفتر الأستاذ (ledger) المحلي بـ:
>
> ```bash
> solana-validator
>   --wait-for-supermajority SLOT_X     # <-- NEW! IMPORTANT! REMOVE AFTER THIS RESTART
>   --expected-bank-hash NEW_BANK_HASH  # <-- NEW! IMPORTANT! REMOVE AFTER THIS RESTART
>   --hard-fork SLOT_X                  # <-- NEW! IMPORTANT! REMOVE AFTER THIS RESTART
>   --no-snapshot-fetch                 # <-- NEW! IMPORTANT! REMOVE AFTER THIS RESTART
>   --entrypoint entrypoint.testnet.solana.com:8001
>   --trusted-validator 5D1fNXzvv5NjV1ysLjirC4WY92RNsVH18vjmcszZd8on
>   --expected-genesis-hash 4uhcVJyU9pJkvQyS88uRDiswHXSCkY3zQawwpjk2NsNY
>   --no-untrusted-rpc
>   --limit-ledger-size
>   ...                                # <-- your other --identity/--vote-account/etc arguments
> ```

````

b. إذا لم يكن لدى المُدقّق (validator) الخاص بك دفتر الأستاذ (ledger) حتى الفُتحة SLOT_X أو إذا قُمت بحذف دفتر الأستاذ الخاص بك، فقُم بتنزيل لقطة (snapshot) بإستخدام:

```bash
solana-validator
  --wait-for-supermajority SLOT_X     # <-- NEW! IMPORTANT! REMOVE AFTER THIS RESTART
  --expected-bank-hash NEW_BANK_HASH  # <-- NEW! IMPORTANT! REMOVE AFTER THIS RESTART
  --entrypoint entrypoint.testnet.solana.com:8001
  --trusted-validator 5D1fNXzvv5NjV1ysLjirC4WY92RNsVH18vjmcszZd8on
  --expected-genesis-hash 4uhcVJyU9pJkvQyS88uRDiswHXSCkY3zQawwpjk2NsNY
  --no-untrusted-rpc
  --limit-ledger-size
  ...                                # <-- your other --identity/--vote-account/etc arguments
````

     يُمكنك التحقق من الفُتحات (clusters) الموجودة في دفتر الأستاذ (ledger) الخاص بك من خلال:`solana-ledger-tool -l path/to/ledger bounds`

3. إنتظر حتى تُصبح 80% من الحِصَّة (stake) مُتصلة بالشبكة

للتأكد من أن المُدقّق (validator) الخاص بك المُعاد تشغيله ينتظر بشكل صحيح الـ 80٪: a. أُنظر إلى رسائل سِجِل الحِصَّة نَشِطة الواضحة في القيل والقال `N% of active stake visible in gossip` b. قُم بالإستعلام من خلال الـ RPC عن الفُتحة (cluster) الموجود عليها: `solana --url http://127.0.0.1:8899 فتحة`. يجب أن تُعيد `SLOT_X` حتى نصل إلى 80٪ من الحِصَّة (stake)

شكراً!

### الخطوة 7. إنتظر وإستمع (Wait and listen)

مراقبة المُدقّقين (validators) عند إعادة التشغيل. الإجابة على الأسئلة، مُساعدة الأشخاص،
