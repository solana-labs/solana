---
title: إدارة حساب التصويت (Vote Account Management)
---

تصف هذه الصفحة كيفية إعداد حساب التصويت _vote account_. على الشبكة (On-Chain). يلزم إنشاء حساب تصويت إذا كنت تُخطط لتشغيل عُقدة تدقيق (valdiator node) على Solana.

## إنشاء حساب تصويت (Create Vote Account)

يُمكن إنشاء حساب تصويت بإستخدام الأمر إنشاء حساب التصويت [create-vote-account](../cli/usage.md#solana-create-vote-account). يُمكن تكوين حساب التصويت عند إنشائه لأول مرة أو بعد تشغيل المُدقّق (validator). يُمكن تغيير جميع جوانب حساب التصويت بإستثناء [vote account address](#vote-account-address)، والذي تم إصلاحه طوال عمر الحساب.

### إعدادات حساب التصويت الحالي (Configure an Existing Vote Account)

- لتغيير هوية المُدقّق [validator identity](#validator-identity)، إستخدم [vote-update-validator](../cli/usage.md#solana-vote-update-validator).
- لتغيير سلطة التصويت [vote authority](#vote-authority)، إستخدم [vote-authorize-voter](../cli/usage.md#solana-vote-authorize-voter).
- لتغيير سلطة السحب [withdraw authority](#withdraw-authority)، إستخدم [vote-authorize-withdrawer](../cli/usage.md#solana-vote-authorize-withdrawer).
- لتغيير العمولة [commission](#commission)، إستخدم [vote-update-commission](../cli/usage.md#solana-vote-update-commission).

## هيكل حساب التصويت (Vote Account Structure)

### عنوان حساب التصويت (Vote Account Structure)

يتم إنشاء حساب تصويت على عنوان يكون إما المفتاح العمومي (public key) لملف زوج المفاتيح (keypair)، أو في عنوان مُشتق بناءً على المفتاح العمومي (public key) لملف زوج المفاتيح (keypair) والسلسلة الأولية (seed string).

لا يلزم أبدًا عنوان حساب التصويت للتوقيع على أي مُعاملات، ولكنه يُستخدم فقط للبحث عن معلومات الحساب.

عندما يرغب شخص ما في تفويض الرموز في حساب حِصَّة [delegate tokens in a stake account](../staking.md)، يتم توجيه أمر التفويض إلى عنوان حساب التصويت الخاص المُدقّق (validator) الذي يريد حامل الرمز (token-holder) تفويضه.

### هوية المُدقّق (Validator Identity)

هوية المُدقّق _validator identity_ هي حساب نظام يتم إستخدامه لدفع جميع رسوم مُعاملات التصويت المُقدمة إلى حساب التصويت. نظرًا لأنه من المُتوقع أن يقوم المُدقّق (validator) بالتصويت على مُعظم الكتل (blocks) الصالحة التي يتلقاها، فإن حساب هوية المُدقّق (validator identity account) غالبًا (من المُحتمل عدة مرات في الثانية) يُوقع المُعاملات ويدفع الرسوم. لهذا السبب، يجب تخزين زوج مفاتيح هوية المُدقّق (validator identity keypair) كـ "محفظة ساخنة" (hot wallet) في ملف زوج مفاتيح على نفس النظام الذي تعمل فيه عملية التدقيق.

نظرًا لأن المحفظة الساخنة (hot wallet) أقل أمانًا بشكل عام من المحفظة غير المُتصلة بالأنترنات أو "المحفظة الباردة" (cold wallet)، فقد يختار عامل المُدقّق (validator) تخزين ما يكفي فقط من عملات SOL على حساب الهوية (identity account) لتغطية رسوم التصويت لفترة زمنية محدودة، مثل بضعة أسابيع أو أشهر. يُمكن أن يتم تفريغ حساب هوية المُدقّق (validator identity account) بشكل دوري من محفظة أكثر أمانًا.

يُمكن أن تُقلل هذه الممارسة من مخاطر فقدان الأموال إذا تعرض قُرص عُقدة التدقيق (validator node) أو نظام الملفات للخطر أو التلف.

يجب توفير هوية المُدقّق (validator identity) عند إنشاء حساب تصويت. يُمكن أيضًا تغيير هوية المُدقّق (validator identity) بعد إنشاء حساب بإستخدام الأمر [vote-update-validator](../cli/usage.md#solana-vote-update-validator).

### سلطة التصويت (Vote Authority)

يتم إستخدام زوج مفاتيح سلطة التصويت _vote authority_ للتوقيع على كل مُعاملة تصويت تريد عُقدة التدقيق (validator node) إرسالها إلى المجموعة (cluster). ليس بالضرورة أن يكون هذا فريدًا من هوية المُدقّق (validator)، كما سترى لاحقًا في هذا المُستند. نظرًا لأن سلطة التصويت، مثل هوية المُدقّق (validator identity)، تقوم بتوقيع المُعاملات بشكل مُتكرر، يجب أن يكون هذا أيضًا زوج مفاتيح ساخنة (hot keypair) على نفس نظام الملفات مثل عملية المُدقّق (validator).

يُمكن تعيين سلطة التصويت على نفس العنوان مثل هوية المُدقّق (validator identity). إذا كانت هوية المُدقّق (validator identity) هي أيضًا سلطة التصويت، فستكون هناك حاجة إلى توقيع واحد فقط لكل مُعاملة تصويت من أجل التوقيع على التصويت ودفع رسوم المُعاملة. نظرًا لأن رسوم المُعاملات على Solana يتم تقييمها لكل توقيع، فإن وجود مُوَقِّع (signer) واحد بدلاً من إثنين سيُؤدي إلى دفع نصف رسوم المُعاملة مُقارنة بتعيين سلطة التصويت وهوية المُدقّق (validator identity) في حسابين مُختلفين.

يُمكن تعيين سلطة التصويت عند إنشاء حساب التصويت. إذا لم يتم توفيره، فسيكون السلوك الإفتراضي هو تعيينه بنفس هوية المُدقّق (validator identity). يُمكن تغيير سلطة التصويت لاحقًا بإستخدام الأمر [vote-authorize-voter](../cli/usage.md#solana-vote-authorize-voter).

يُمكن تغيير سلطة التصويت مرة واحدة على الأكثر في كل فترة (epoch). إذا تم تغيير السلطة بـ [vote-authorize-voter](../cli/usage.md#solana-vote-authorize-voter)، فلن يُصبح هذا ساري المفعول حتى بداية الفترة (epoch) التالية. لدعم الإنتقال السلس لتوقيع التصويت، يسمح `solana-validator` بتحديد الوسيطة `--authorized-voter` عدة مرات. يسمح هذا لعملية المُدقّق (validator) بمُواصلة التصويت بنجاح عندما تصل الشبكة إلى حد الفترة (epoch) التي يتغير فيها حساب سلطة التصويت للمُدقّق (validator).

### سلطة سحب الأموال (Withdraw Authority)

يتم إستخدام زوج مفاتيح سلطة سحب الأموال _withdraw authority_ من حساب التصويت بإستخدام الأمر سحب الأموال من حساب التصويت [withdraw-from-vote-account](../cli/usage.md#solana-withdraw-from-vote-account). يتم إيداع أي مُكافآت في الشبكة يكسبها المُدقّق (validator) في حساب التصويت ولا يُمكن إستردادها إلا من خلال التوقيع مع زوج مفاتيح سلطة السحب (withdraw authority keypair).

سلطة سحب الأموال مطلوبة أيضًا للتوقيع على أي مُعاملة لتغيير عمولة [commission](#commission) حساب تصويت، ولتغيير هوية المُدقّق (validator identity) على حساب التصويت.

نظرًا لأن حساب التصويت يُمكن أن يُراكم رصيدًا كبيرًا، ففكر في الإحتفاظ بزوج مفاتيح سلطة سحب الأموال (withdraw authority keypair) في محفظة غير مُتصلة بالأنترنات / باردة، حيث لا يلزم توقيع مُعاملات متكررة.

يُمكن تعيين سلطة السحب عند إنشاء حساب التصويت بخيار الساحب المُصرَّح به `--authorized-withdrawer`. إذا لم يتم توفير ذلك، فسيتم تعيين هوية المُدقّق (validator identity) كسلطة سحب أموال بشكل إفتراضي.

يُمكن تغيير سلطة السحب لاحقًا بإستخدام الأمر التصويت بالتصريح للساحب [vote-authorize-withdrawer](../cli/usage.md#solana-vote-authorize-withdrawer).

### العمولة (Commission)

العمولة _Commission_ هي النسبة المئوية لمُكافآت الشبكة التي يربحها المُدقّق (validator) والتي يتم إيداعها في حساب تصويت المُدقّق. يتم توزيع ما تبقى من المُكافآت على جميع حسابات الأسهم المُفوضة لحساب التصويت هذا، بما يتناسب مع وزن الحِصَّة (stake) النشطة لكل حساب حِصَّة.

على سبيل المثال، إذا كان حساب التصويت به عمولة بنسبة 10٪، بالنسبة لجميع المُكافآت التي حصل عليها هذا المُدقّق (validator) في فترة مُعينة، فسيتم إيداع 10٪ من هذه المُكافآت في حساب التصويت في الكتلة الأولى من الفترة (epoch) التالية. سيتم إيداع الـ 90٪ المُتبقية في حسابات حِصَص (stakes) مُفوضة كحِصَة (stake) نشطة على الفور.

قد يختار المُدقّق (validator) تعيين عمولة مُنخفضة لمحاولة جذب المزيد من تفويضات الحِصَّة (stake) حيث ينتج عن العمولة الأقل نسبة أكبر من المُكافآت التي يتم تمريرها إلى المُفوِّض (delegator). نظرًا لوجود تكاليف مُرتبطة بإعداد وتشغيل عُقدة التدقيق (validator node) من الصحة، فإن المُدقّق (validator) سيُحدد بشكل مثالي عمولة عالية بما يكفي لتغطية نفقاتهم على الأقل.

يُمكن تعيين العمولة عند إنشاء حساب التصويت بخيار العمولة `--commission`. إذا لم يتم توفيره، فسيتم تعيينه إفتراضيًا إلى 100٪، مما سيُؤدي إلى إيداع جميع المُكافآت في حساب التصويت، ولن يتم تمرير أي منها إلى أي حسابات حسابات تحْصِيص مُفوضة (delegated stake accounts).

يُمكن أيضًا تغيير العمولة لاحقًا بإستخدام الأمر عمولة تحديث التصويت [vote-update-commission](../cli/usage.md#solana-vote-update-commission).

عند تعيين العمولة، يتم قبول قِيم الأعداد الصحيحة (integer values) فقط في المجموعة [0-100]. يُمثل العدد الصحيح (integer) عدد النقاط المئوية للعمولة، لذا فإن إنشاء حساب بإستخدام `--commission 10` سيُحدد عمولة بنسبة 10٪.

## تناوب المفتاح (Key Rotation)

يتطلب تناوب مفاتيح سلطة حساب التصويت مُعالجة خاصة عند التعامل مع مُدقّق (validator) شغال.

### التصويت على حساب هوية المُدقّق (Vote Account Validator Identity)

ستحتاج إلى الوصول إلى زوج مفاتيح سلطة سحب الأموال _withdraw authority_ لحساب التصويت لتغيير هوية المُدقّق (validator). تفترض خطوات المُتابعة أن `~/withdraw-authority.json` هو زوج المفاتيح (keypair) هذا.

1. قُم بإنشاء زوج مفاتيح هوية المُدقّق (validator identity keypair) الجديد، `solana-keygen new -o ~/new-validator-keypair.json`.
2. تأكد من أن حساب الهوية الجديد قد تم تمويله، `solana transfer ~/new-validator-keypair.json 500`.
3. قُم بتشغيل `solana vote-update-validator ~/vote-account-keypair.json ~/new-validator-keypair.json ~/withdraw-authority.json` لتعديل هوية المُدقّق (validator identity) في حساب التصويت الخاص بك
4. أعد تشغيل المُدقّق (validator) بإستخدام زوج مفاتيح الهوية الجديد للوسيطة `--identity`

### الناخب المُفوض لحساب التصويت (Vote Account Authorized Voter)

لا يُمكن تغيير زوج المفاتيح _vote authority_ إلا عند حدود الفترة (epoch) ويتطلب بعض الوسيطات الإضافية لـ `solana-validator` لترحيل سلس.

1. قُم بتشغيل `solana epoch-info`. إذا لم يكن هناك الكثير من الوقت المُتبقي في الفترة (epoch) الحالية، ففكر في إنتظار الفترة (epoch) التالية للسماح للمُدقّق (validator) الخاص بك بوقت كافٍ لإعادة التشغيل واللحاق بالركب.
2. إنشاء زوج مفاتيح السلطة (authority keypair) التصويت الجديد `solana-keygen new -o ~/new-vote-authority.json`.
3. Determine the current _vote authority_ keypair by running `solana vote-account ~/vote-account-keypair.json`. قد يكون حساب هوية المُدقّق (validator) (الإفتراضي) أو بعض أزواج المفاتيح (keypair) الأخرى. تفترض الخطوات التالية أن `~/validator-keypair.json` هو زوج المفاتيح (keypair) هذا.
4. قُم بتشغيل `solana vote-authorize-voter ~/vote-account-keypair.json ~/validator-keypair.json ~/new-vote-authority.json`. من المُقرر أن تُصبح سلطة التصويت الجديدة نشطة إعتبارًا من الفترة (epoch) التالية.
5. يحتاج `solana-validator` الآن إلى إعادة التشغيل بإستخدام زوج مفاتيح سلطة التصويت (vote authority keypairs) القديمة والجديدة، حتى يتمكن من الإنتقال بسلاسة في الفترة (epoch) التالية. Add the two arguments on restart: `--authorized-voter ~/validator-keypair.json --authorized-voter ~/new-vote-authority.json`
6. بعد وصول المجموعة إلى المرحلة التالية، قُم بإزالة الوسيطة `--authorized-voter ~/validator-keypair.json` وأعد تشغيل `solana-validator`، حيث لم يعد زوج مفاتيح سلطة التصويت (vote authority keypairs) القديم مطلوبًا.

### ساحب حساب التصويت المُصرح (Vote Account Authorized Withdrawer)

لا يتطلب مُعالجة خاصة. إستخدم الأمر `solana vote-authorize-withdrawer` حسب الحاجة.
