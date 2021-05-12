---
title: "التطوير مع اللغة البرمجية C"
---

تدعم Solana كتابة البرامج على الشبكة (On-Chain) بإستخدام لغات البرمجة C و C ++.

## تخطيط المشروع

تم وضع مشاريع اللغة البرمجية C على النحو التالي:

```
/src/<program name>/makefile
```

يجب أن يحتوي `makefile` على ما يلي:

```bash
OUT_DIR := <path to place to resulting shared object>
include ~/.local/share/solana/install/active_release/bin/sdk/bpf/c/bpf.mk
```

قد لا يكون bpf-sdk في المكان المحدد أعلاه ولكن إذا قمت بإعداد بيئتك حسب [How to Build](#how-to-build)، فيجب أن تكون كذلك.

ألقي نظرة على [helloworld](https://github.com/solana-labs/example-helloworld/tree/master/src/program-c) للحصول على مثال لبرنامج C.

## كيفية البناء والتطوير

قم أولاً بإعداد البيئة:

- قم بتثبيت أحدث برنامج Rust مستقر من https://rustup.rs
- قم بتثبيت أحدث أدوات سطر الأوامر (command-line tools) الخاصة بSolana من https://docs.solana.com/cli/install-solana-cli-tools

ثم قم بالبناء بإستخدام صنع أو make:

```bash
make -C <program directory>
```

## كيفية الإختبار

تستخدم Solana إطار عمل الإختبار [Criterion](https://github.com/Snaipe/Criterion) ويتم تنفيذ الإختبارات في كل مرة يتم فيها إنشاء البرنامج [How to Build](#how-to-build)].

لإضافة إختبارات، قم بإنشاء ملف جديد بجوار الملف المصدر الخاص بك والمسمى `test_<program name>.c` وقم بتعبئته بحالات الإختبار المعيارية. على سبيل المثال، انظر [helloworld C tests](https://github.com/solana-labs/example-helloworld/blob/master/src/program-c/src/helloworld/test_helloworld.c) أو [Criterion docs](https://criterion.readthedocs.io/en/master) للحصول على معلومات عن كيفية كتابة حالة إختبار.

## نقطة دخول البرنامج (Program Entrypoint)

تقوم البرامج بتصدير رمز نقطة دخول معروف يبحث عنه وقت تشغيل Solana ويستدعيه عند إستدعاء أحد البرامج. تدعم Solana عدة إصدارات [versions of the BPF loader](overview.md#versions) وقد تختلف نقاط الإدخال فيما بينها. يجب كتابة البرامج ونشرها على نفس المُحمل (loader). لمزيد من التفاصيل، راجع اللمحة العامة [overview](overview#loaders).

يوجد حاليًا مُحملان (loaders) إثنان مدعومان من أجهزة التحميل المدعومة [BPBPF Loader](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/bpf_loader.rs#L17)و [BPF loader deprecated](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/bpf_loader_deprecated.rs#L14)

كلاهما لهما نفس تعريف نقطة الدخول الأولية، ما يلي هو الرمز الأولي / الخام الذي يبحث عنه وقت التشغيل ويستدعي:

```c
extern uint64_t entrypoint(const uint8_t *input)
```

تأخذ نقطة الدخول هذه مصفوفة byte عامة (generic byte array) تحتوي على مُعلمات (parameters) البرنامج المتسلسلة (معرف البرنامج، الحسابات، بيانات التعليمات، إلخ...). لإلغاء تسلسل المُعلمات (parameters)، يحتوي كل مُحمل (loader) على [helper function](#Serialization).

راجع [helloworld's use of the entrypoint](https://github.com/solana-labs/example-helloworld/blob/bc0b25c0ccebeff44df9760ddb97011558b7d234/src/program-c/src/helloworld/helloworld.c#L37) كمثال على كيفية التوفيق بين الأشياء.

### التسلسل (Serialization)

راجع [helloworld's use of the deserialization function](https://github.com/solana-labs/example-helloworld/blob/bc0b25c0ccebeff44df9760ddb97011558b7d234/src/program-c/src/helloworld/helloworld.c#L43).

يوفر كل مُحمل (loader) وظيفة مساعد تقوم بإلغاء تسلسل معلِّمات (parameters) البرنامج في الأنواع C:

- [إلغاء تسلسل مُحمل BPF](https://github.com/solana-labs/solana/blob/d2ee9db2143859fa5dc26b15ee6da9c25cc0429c/sdk/bpf/c/inc/solana_sdk.h#L304)
- [إيقاف إلغاء التسلسل لمُحمل BPF](https://github.com/solana-labs/solana/blob/8415c22b593f164020adc7afe782e8041d756ddf/sdk/bpf/c/inc/deserialize_deprecated.h#L25)

قد ترغب بعض البرامج في إلغاء التسلسل بأنفسهم ويمكنهم ذلك من خلال توفير التنفيذ الخاص بهم [raw entrypoint](#program-entrypoint). لاحظ أن وظائف إلغاء التسلسل المقدمة تحتفظ بالمراجع إلى صفيف الbyte المُتسلسل (serialized byte array) للمتغيرات (variables) التي يُسمح للبرنامج بتعديلها (lamports، بيانات الحساب). السبب في ذلك هو أنه عند الإرجاع، سيقرأ المُحمل (loader) تلك التعديلات حتى يتم الإلتزام بها. إذا قام أحد البرامج بتنفيذ وظيفة إلغاء التسلسل الخاصة به، فإنه يحتاج إلى التأكد من إعادة كتابة أي تعديلات يرغب البرنامج في الإلتزام بها في مصفوفة مجموعة byte الإدخال ( input byte array).

تفاصيل عن كيفية تسلسل المُحمل (loader) لمدخلات البرنامج يمكن العثور عليها في مستندات [Input Parameter Serialization](overview.md#input-parameter-serialization).

## أنواع البيانات

تعمل وظيفة مساعد مُحمل (loader) إلغاء التسلسل على ملء [SolParameters](https://github.com/solana-labs/solana/blob/8415c22b593f164020adc7afe782e8041d756ddf/sdk/bpf/c/inc/solana_sdk.h#L276) الهيكل:

```c
/**
 * Structure that the program's entrypoint input data is deserialized into.
 */
typedef struct {
  SolAccountInfo* ka; /** Pointer to an array of SolAccountInfo, must already
                          point to an array of SolAccountInfos */
  uint64_t ka_num; /** Number of SolAccountInfo entries in `ka` */
  const uint8_t *data; /** pointer to the instruction data */
  uint64_t data_len; /** Length in bytes of the instruction data */
  const SolPubkey *program_id; /** program_id of the currently executing program */
} SolParameters;
```

'ka' هي مجموعة مرتبة من الحسابات المشار إليها بالتعليمات وتم تمثيلها على أنها [SolAccountInfo](https://github.com/solana-labs/solana/blob/8415c22b593f164020adc7afe782e8041d756ddf/sdk/bpf/c/inc/solana_sdk.h#L173) بنى. يشير مكان الحساب في المصفوفة إلى معناها، على سبيل المثال، عند نقل lamports، قد تحدد التعليمات الحساب الأول بإعتباره المصدر والثاني كوجهة.

أعضاء الهيكل `SolAccountInfo` للقراءة فقط (read-only) بإستثناء `lamports` و `data`. يمكن تعديل كليهما بواسطة البرنامج وفقًا لـ[runtime enforcement policy](developing/programming-model/accounts.md#policy). عندما تشير إحدى التعليمات إلى نفس الحساب عدة مرات، فقد يكون هناك إدخالات `SolAccountInfo` مكررة في المصفوفة ولكن كلاهما يشير إلى مصفوفة الإدخال byte الأصلية. يجب أن يتعامل البرنامج مع هذه الحالة بدقة لتجنب تداخل القراءة / الكتابة في نفس المخزن المؤقت (buffer). إذا قام أحد البرامج بتنفيذ وظيفة إلغاء التسلسل الخاصة به، فيجب توخي الحذر بشكل مناسب في التعامل مع الحسابات المكررة.

`data` هي مجموعة الbyte للأغراض العامة من [instruction's instruction data](developing/programming-model/transactions.md#instruction-data) التي تتم معالجتها.

مُعرف البرنامج `program_id` هو المفتاح العمومي (public key) لبرنامج التنفيذ الحالي.

## كومة الذاكرة المؤقتة (Heap)

يمكن لبرامج C تخصيص الذاكرة عن طريق إستدعاء النظام [`calloc`](https://github.com/solana-labs/solana/blob/c3d2d2134c93001566e1e56f691582f379b5ae55/sdk/bpf/c/inc/solana_sdk.h#L245) أو تنفيذ كومة الذاكرة المؤقتة (heap) الخاصة بهم أعلى منطقة كومة الذاكرة المؤقتة 32KB بدءًا من العنوان الظاهري x300000000. يتم إستخدام منطقة كومة الذاكرة المؤقتة (heap) أيضًا بواسطة `calloc` لذلك إذا قام أحد البرامج بتنفيذ كومة ذاكرة مؤقتة (heap) خاصة به، فلا يجب أن يستدعي `calloc` أيضًا.

## التسجيل

يوفر وقت التشغيل مكالمتين نظاميتين تأخذ البيانات وتسجلها في سجلات البرنامج.

- [`sol_log(const char*)`](https://github.com/solana-labs/solana/blob/d2ee9db2143859fa5dc26b15ee6da9c25cc0429c/sdk/bpf/c/inc/solana_sdk.h#L128)
- [`sol_log_64(uint64_t, uint64_t, uint64_t, uint64_t, uint64_t)`](https://github.com/solana-labs/solana/blob/d2ee9db2143859fa5dc26b15ee6da9c25cc0429c/sdk/bpf/c/inc/solana_sdk.h#L134)

يحتوي قسم [debugging](debugging.md#logging) على مزيد من المعلومات حول العمل مع سجلات البرامج.

## حساب الميزانية (Compute Budget)

إستخدم إستدعاء النظام [`sol_log_compute_units()`](https://github.com/solana-labs/solana/blob/d3a3a7548c857f26ec2cb10e270da72d373020ec/sdk/bpf/c/inc/solana_sdk.h#L140) لتسجيل رسالة تحتوي على العدد المتبقي من وحدات الحساب التي قد يستهلكها البرنامج قبل إيقاف التنفيذ

See [compute budget](developing/programming-model/runtime.md#compute-budget) for more information.

## تفريغ ELF أو ELF Dump

يمكن تفريغ العناصر الداخلية للكائن المشترك BPF في ملف نصي للحصول على نظرة ثاقبة أكثر في تكوين البرنامج وما يمكن أن يفعله في وقت التشغيل. سوف يحتوي على معلومات ELF وكذلك على قائمة بجميع الرموز والتعليمات التي تنفذها. بعض رسائل مُحمل BPF سوف تشير إلى أرقام تعليمات محددة حيث حدث الخطأ. يمكن البحث عن هذه الإشارات في ملفات تفريغ ELF لتحديد التعليمات المسيئة وسياقها.

لإنشاء ملف تفريغ (dump):

```bash
$ cd <program directory>
$ make dump_<program name>
```

## أمثلة

يحتوي مستودع [Solana Program Library github](https://github.com/solana-labs/solana-program-library/tree/master/examples/c) على مجموعة من أمثلة اللغة البرمجية C
