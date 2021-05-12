---
title: بيانات مجموعة Sysvar
---

تعرض Solana مجموعة متنوعة من بيانات حالة المجموعة (cluster) للبرامج من خلال حسابات [`sysvar`](terminology.md#sysvar). هذه الحسابات مأهولة في عناوين معروفة منشورة جنبا إلى جنب مع مخططات الحساب في [`برنامج-solana`صندق](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/sysvar/index.html)، والمُبينة أدناه.

لتضمين بيانات sysvar في عمليات البرنامج، قم بتمرير عنوان حساب sysvar في قائمة الحسابات في المعاملة. يمكن قراءة الحساب في معالج التعليمات الخاص بك مثل أي حساب آخر. Access to sysvars accounts is always _readonly_.

## الساعة (Clock)

يحتوي نظام ساعة sysvar على بيانات عن وقت المجموعة (cluster)، بما في ذلك الفتحة (slot) الحالية، الفترة (epoch) والختم الزمني (Timestamp) المُقدر لساعة حائط Unix. يتم تحديثه في كل فتحة (slot).

- العنوان: `SysvarC1ock11111111111111111111111111111111111111111111111111111111`
- التخطيط: [Clock](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/clock/struct.Clock.html)
- الحقول (Fields):

  - `فتحة`: الفتحة (slot) الحالية
  - `epoch_start_timestamp`: الختم الزمني (Timestamp) للفتحة الأولى في هذه الفترة (epoch). في الفتحة (slot) الأولى من الفترة (epoch)، هذا الختم الزمني (Timestamp) مطابق `unix_timestamp` (أدناه).
  - `الفترة`: الفترة (epoch) الحالية
  - `Lead_schedule_epoch`: آخر فترة (epoch) تم فيها إنشاء جدول القائد (leader schedule)
  - `unix_timestamp`: الختم الزمني (Timestamp) لهذه الفتحة (slot).

  لكل فتحة (slot) مدة تقديرية تستند إلى إثبات التاريخ (Proof of History). لكن في الواقع، الفتحات (slots) قد تنقضي أسرع وأبطأ من هذا التقدير. نتيجة لذلك، يتم إنشاء الختم الزمني (Timestamp) لـ Unix من الفتحة (slot) إستنادًا إلى مُدخلات oracle من مُدققي التصويت (voting validators). يتم حساب هذا الختم الزمني (Timestamp) كمتوسط مرجح حسب الحِصَة (stake-weighted) لتقديرات الأختام الزمنية (Timestamps) المقدمة من الأصوات، مقيدة بالوقت المتوقع الذي إنقضى منذ بدء الفترة (epoch).

  على نحو أكثر وضوحا: لكل فتحة (slot)، آخر وقت للتصويت مقدم من كل مدقق يستخدم لإنشاء تقدير زمني للفتحة الحالية (الفتحات التي انقضت منذ الافتراض أن الوقت للتصويت هو Bank::ns_per_slot). بشكل أكثر وضوحًا: بالنسبة لكل (slot)، يتم إستخدام أحدث ختم زمني (Timestamp) للتصويت الأخير الذي تم توفيره بواسطة كل مدقق (validator) لتوليد تقدير ختم زمني (Timestamp) للفتحة (slot) الحالية (يُفترض أن تكون الفتحات المنقضية منذ الختم الزمني للتصويت Bank:: ns_per_slot). كل تقدير للختم الزمني (Timestamp) مرتبط بالحِصَة (stake) المفوضة إلى حساب التصويت (vote account) لإنشاء توزيع الأختام الزمنية (Timestamps) حسب الحِصَة (stake). يستخدم الوسط الختم الزمني (Timestamp) كـ `unix_timestamp`، ما لم ينحرف الوقت منذ `epoch_start_timestamp` عن الوقت المتوقع بأكثر من 25%.

## جدول الفترة (EpochSchedule)

The EpochSchedule sysvar contains epoch scheduling constants that are set in genesis, and enables calculating the number of slots in a given epoch, the epoch for a given slot, etc. (Note: the epoch schedule is distinct from the [`leader schedule`](terminology.md#leader-schedule))

- العنوان: `SysvarC1ock11111111111111111111111111111111111111111111111111111111`
- التخطيط: جدول الفترة [EpochSchedule](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/epoch_schedule/struct.EpochSchedule.html)

## الرسوم (Fees)

تحتوي رسوم sysvar على آلة حاسبة الرسوم للفتحة (Slot) الحالية. يتم تحديث كل فتحة (Slot) إستنادا إلى مُحافظ مُعدل الرسوم (fee-rate governor).

- العنوان: `SysvarC1ock11111111111111111111111111111111111111111111111111111111`
- التخطيط: الرسوم [Fees](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/sysvar/fees/struct.Fees.html)

## التعليمات (Instructions)

يحتوي نظام التعليمات sysvar على التعليمات المتسلسلة في رسالة بينما يتم معالجة هذه الرسالة. يسمح هذا لتعليمات البرنامج بالرجوع إلى تعليمات أخرى في نفس المعاملة. إقرأ المزيد من المعلومات عن البحث الذاتي في التعليمات [instruction introspection](implemented-proposals/instruction_introspection.md).

- العنوان: `SysvarC1ock11111111111111111111111111111111111111111111111111111111`
- التخطيط: التعليمات [Instructions](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/sysvar/instructions/struct.Instructions.html)

## تجزئة الكتلة الحديثة (RecentBlockhash)

يحتوي متغير النظام (system variable أو sysvar) المسمى RecentBlockhashes على تجزئة الكتلة (blockhashes) الحديثة بالإضافة إلى آلة حاسبة الرسوم المرتبطة بها. يتم تحديثه في كل فتحة (slot).

- العنوان: `SysvarC1ock11111111111111111111111111111111111111111111111111111111`
- التخطيط: تجزئة الكتلة الحديثة [RecentBlockhashes](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/sysvar/recent_blockhashes/struct.RecentBlockhashes.html)

## التأجير (Rent)

يحتوي إيجار مُتغير النظام (Rent sysvar) على مُعدل الإيجار. في الوقت الحالي، المُعدل ثابت ومُحَدَّدَ في مرحلة التكوين (genesis). يتم تعديل نسبة حرق الإيجار من خلال تفعيل الميزة اليدوية.

- العنوان: `SysvarRent111111111111111111111111111111111`
- التخطيط: الإيجار [Rent](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/rent/struct.Rent.html)

## تجزئات الفُتحة (SlotHashes)

يحتوي مُتغير النظام (sysvar) المُسَمَّى تجزئات الفُتحة (SlotHashes) على أحدث تجزئات للبنوك الأصلية للفُتحة (slot). يتم تحديثه في كل فُتحة (slot).

- العنوان: `SysvarS1otHashes111111111111111111111111111`
- التخطيط: تجزئات الفُتحة [SlotHashes](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/slot_hashes/struct.SlotHashes.html)

## تاريخ الفُتحة (SlotHistory)

مُتغير النظام (sysvar) المُسَمَّى تاريخ الفُتحة (SlotHistory) يحتوي على مقطع من الفُتحات (slots) الموجودة على آخر فترة (epoch). يتم تحديثه في كل فتحة (slot).

- العنوان: `SysvarS1otHistory11111111111111111111111111`
- التخطيط: تاريخ الفتحة [SlotHistory](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/slot_history/struct.SlotHistory.html)

## تاريخ الحِصَّة (StakeHistory)

يحتوي متغير النظام (system variable أو sysvar) المسمى تاريخ الحِصَّة (StakeHistory) على تاريخ تنشيط الحِصَّة (Stake) على نطاق المجموعة (cluster-wide) وإلغاء التنشيط لكل فترة (epoch). ويجري تحديثها في بداية كل فترة (epoch).

- العنوان: `SysvarC1ock11111111111111111111111111111111111111111111111111111111`
- التخطيط: تاريخ الحصة [StakeHistory](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/stake_history/struct.StakeHistory.html)
