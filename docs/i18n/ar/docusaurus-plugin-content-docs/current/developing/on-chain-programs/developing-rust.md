---
title: "التطوير مع اللغة البرمجية C"
---

تدعم Solana كتابة البرامج على الشبكة (on-chain) بإستخدام لغة البرمجة [Rust](https://www.rust-lang.org/).

## تخطيط المشروع

تتبع اللغة البرمجية Rust في Solana نسق نموذجي [Rust project layout](https://doc.rust-lang.org/cargo/guide/project-layout.html):

```
/inc/
/src/
/Cargo.toml
```

لكن يجب أن تشمل أيضا ما يلي:

```
/Xargo.toml
```

الذي يجب أن يحتوي على:

```
[target.bpfel-unknown-unknown.dependencies.std]
features = []
```

Solana Rust programs may depend directly on each other in order to gain access to instruction helpers when making [cross-program invocations](developing/programming-model/calling-between-programs.md#cross-program-invocations). عند القيام بذلك، من المهم عدم سحب رموز نقطة دخول البرنامج التابع لها لأنها قد تتعارض مع رموز البرنامج. To avoid this, programs should define an `exclude_entrypoint` feature in `Cargo.toml` and use to exclude the entrypoint.

- [تحديد الميزة](https://github.com/solana-labs/solana-program-library/blob/a5babd6cbea0d3f29d8c57d2ecbbd2a2bd59c8a9/token/program/Cargo.toml#L12)
- [إستبعاد نقطة الدخول](https://github.com/solana-labs/solana-program-library/blob/a5babd6cbea0d3f29d8c57d2ecbbd2a2bd59c8a9/token/program/src/lib.rs#L12)

ثم عندما تتضمن البرامج الأخرى هذا البرنامج بإعتباره تبعية، يجب عليهم القيام بذلك بإستخدام الميزة `exclude_entrypoint`.

- [تضمين بدون نقطة دخول](https://github.com/solana-labs/solana-program-library/blob/a5babd6cbea0d3f29d8c57d2ecbbd2a2bd59c8a9/token-swap/program/Cargo.toml#L19)

## تبعيات (Dependencies) المشروع

كحد أدنى، يجب أن تجتذب اللغة البرمجية Rust في Solana في صندوق [solana-program](https://crates.io/crates/solana-program).

تحتوي برامج Solana BPF على بعض القيود [restrictions](#restrictions) التي قد تمنع إدراج بعض الصناديق كإعتمادات أو تتطلب معالجة خاصة.

على سبيل المثال:

- الصناديق التي تتطلب البنية هي مجموعة فرعية من الصناديق التي تدعمها سلسلة الأدوات الرسمية. لا يوجد حل بديل لهذا ما لم يكن هذا الصندوق نتيجة إنقسام / شوكة (forked) وإضافة BPF إلى فحوصات تلك البنية.
- قد تعتمد الصناديق على `rand` الغير مدعوم في بيئة برنامج Solana الحتمية. لتضمين `rand` صندوق تابع يرجى الرجوع إلى [Depending on Rand](#depending-on-rand).
- قد تتدفق الصناديق (crates) على المُكدس (stack) حتى إذا لم يتم تضمين رمز تجاوز المُكدس في البرنامج نفسه. للحصول على مزيد من المعلومات، يرجى الرجوع إلى [Stack](overview.md#stack).

## كيفية البناء والتطوير

قم أولاً بإعداد البيئة:

- قم بتثبيت أحدث برنامج Rust مستقر من https://rustup.rs/
- قم بتثبيت أحدث أدوات سطر الأوامر (command-line tools) الخاصة بSolana من https://docs.solana.com/cli/install-solana-cli-tools

بناء الشحنة العادي متوفر لبرامج بناء مقابل الجهاز المضيف الخاص بك والذي يمكن إستخدامها لإختبار الوحدة:

```bash
$ cargo build
```

لإنشاء برنامج محدد، مثل رمز SPL، لهدف BPF الخاص بSolana الذي يمكن نشره في المجموعة (cluster):

```bash
$ cd <the program directory>
$ cargo build-bpf
```

## كيفية الإختبار

يمكن إختبار برامج Solana من خلال آلية `cargo test` التقليدية من خلال ممارسة وظائف البرنامج مباشرة.

للمساعدة في تسهيل الإختبار في بيئة تتطابق بشكل أكبر مع الفتحة (cluster) الشغالة، يمكن للمطورين إستخدام الصندوق [`program-test`](https://crates.io/crates/solana-program-test). يبدأ الصندوق `program-test` مثيلًا (instance) محليًا لوقت التشغيل ويسمح للإختبارات بإرسال معاملات متعددة مع الحفاظ على الحالة طوال مدة الإختبار.

للحصول على مزيد من المعلومات، يوضح مثال [test in sysvar example](https://github.com/solana-labs/solana-program-library/blob/master/examples/rust/sysvar/tests/functional.rs) كيفية إرسال تعليمات تحتوي على حساب syavar ومعالجتها بواسطة البرنامج.

## نقطة دخول البرنامج (Program Entrypoint)

تقوم البرامج بتصدير رمز نقطة دخول معروف يبحث عنه وقت تشغيل Solana ويستدعيه عند استدعاء أحد البرامج. تدعم Solana إصدارات متعددة [versions of the BPF loader](overview.md#versions) وقد تختلف نقاط الدخول فيما بينها. يجب كتابة البرامج ونشرها على نفس المُحمل (loader). لمزيد من التفاصيل، راجع اللمحة العامة [overview](overview#loaders).

يوجد حاليًا مُحملان (loaders) إثنان مدعومان من أجهزة التحميل المدعومة [BPBPF Loader](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/bpf_loader.rs#L17)و [BPF loader deprecated](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/bpf_loader_deprecated.rs#L14)

كلاهما لهما نفس تعريف نقطة الدخول الأولية، ما يلي هو الرمز الأولي / الخام الذي يبحث عنه وقت التشغيل ويستدعي:

```rust
#[no_mangle]
pub unsafe extern "C" fn entrypoint(input: *mut u8) -> u64;
```

تأخذ نقطة الدخول هذه مصفوفة byte عامة (generic byte array) تحتوي على مُعلمات (parameters) البرنامج المتسلسلة (معرف البرنامج، الحسابات، بيانات التعليمات، إلخ...). لإلغاء تسلسل المُعلمات (parameters)، يحتوي كل مُحمل (loader) على غلاف ماكرو (wrapper macro) خاص به يُصدر نقطة الإدخال الأولية، ويُفكك تسلسل المُعلمات، ويستدعي وظيفة معالجة التعليمات المحددة من قبل المُستخدم، ويعيد النتائج.

يمكنك العثور على نقطة دخول الماكروز (macros) هنا:

- [ماكرو (macro) نقطة دخول مُحمل BPF](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/entrypoint.rs#L46)
- [إلغاء ماكرو (macro) نقطة الدخول الخاصة بمُحمل BPF](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/entrypoint_deprecated.rs#L37)

حدد البرنامج وظيفة معالجة التعليمات التي يجب أن يكون إستدعاء وحدات الماكروز (macros) في نقطة الدخول بهذا النموذج:

```rust
pub type ProcessInstruction =
    fn(program_id: &Pubkey, accounts: &[AccountInfo], instruction_data: &[u8]) -> ProgramResult;
```

راجع [helloworld's use of the entrypoint](https://github.com/solana-labs/example-helloworld/blob/c1a7247d87cd045f574ed49aec5d160aefc45cf2/src/program-rust/src/lib.rs#L15) كمثال على كيفية التوفيق بين الأشياء.

### إلغاء تسلسل المُعلِّمة (parameter)

يوفر كل مُحمل (loader) وظيفة مساعد تقوم بإلغاء تسلسل معلِّمات (parameters) مُدخلات (input) البرنامج في الأنواع Rust. تستدعي نقطة الدخول وحدات الماكروز (macros) مساعد إلغاء التسلسل تلقائيًا:

- [إلغاء تسلسل مُحمل BPF](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/entrypoint.rs#L104)
- [إلغاء التسلسل لمُحمل BPF](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/entrypoint_deprecated.rs#L56)

قد ترغب بعض البرامج في إلغاء التسلسل بنفسها ويمكنها من خلال توفير التنفيذ الخاص بها لـ[raw entrypoint](#program-entrypoint). لاحظ أن وظائف إلغاء التسلسل المقدمة تحتفظ بالمراجع إلى صفيف الbyte المُتسلسل (serialized byte array) للمتغيرات (variables) التي يُسمح للبرنامج بتعديلها (lamports، بيانات الحساب). السبب في ذلك هو أنه عند الإرجاع، سيقرأ المُحمل (loader) تلك التعديلات حتى يتم الإلتزام بها. إذا قام أحد البرامج بتنفيذ وظيفة إلغاء التسلسل الخاصة به، فإنه يحتاج إلى التأكد من إعادة كتابة أي تعديلات يرغب البرنامج في الإلتزام بها في مُدخل (input) مصفوفة مجموعة الbyte.

تفاصيل عن كيفية تسلسل المُحمل (loader) لمُدخلات (inputs) البرنامج يمكن العثور عليها في مستندات [Input Parameter Serialization](overview.md#input-parameter-serialization).

### أنواع البيانات

تستدعي نقطة دخول مُحمل (loader) الماكروز (macros) وظيفة معالج التعليمات المُحددة من قبل البرنامج بالمُعطيات التالية:

```rust
program_id: &Pubkey,
accounts: &[AccountInfo],
instruction_data: &[u8]
```

معرف البرنامج هو المفتاح العمومي (public key) لبرنامج التنفيذ الحالي.

الحسابات عبارة عن شريحة مُرتبة من الحسابات المُشار إليها بالتعليمات وتم تمثيلها على أنها هياكل [AccountInfo](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/account_info.rs#L10). يُشير مكان الحساب في المصفوفة إلى معناها، على سبيل المثال، عند نقل الlamports، قد تُحدد التعليمات الحساب الأول بإعتباره المصدر والثاني كوجهة.

أعضاء بنية `AccountInfo` للقراءة فقط بإستثناء `lamports` و `data`. يمكن تعديل كليهما بواسطة البرنامج وفقًا لـ [runtime enforcement policy](developing/programming-model/accounts.md#policy). كلا هذين العضوين محميان ببناء `RefCell` اللغة البرمجية Rust، لذلك يجب إستعارتهما للقراءة أو الكتابة إليهما. السبب في ذلك هو أن كلاهما يُشير إلى مُدخل مصفوفة byte الأصلية (original input byte array)، ولكن قد تكون هناك مُدخلات مُتعددة في شريحة الحسابات تشير إلى نفس الحساب. يضمن إستخدام `RefCell` أن البرنامج لا يؤدي بطريق الخطأ قراءة / كتابة مُتداخلة لنفس البيانات الأساسية عبر هياكل `AccountInfo` متعددة. إذا قام أحد البرامج بتنفيذ وظيفة إلغاء التسلسل (deserialization) الخاصة به، فيجب توخي الحذر في التعامل مع الحسابات المكررة بشكل مناسب.

بيانات التعليمات هي مصفوفة byte للأغراض العامة من [instruction's instruction data](developing/programming-model/transactions.md#instruction-data) قيد المعالجة.

## كومة الذاكرة المؤقتة (Heap)

تقوم برامج Rust بتنفيذ كومة الذاكرة المؤقتة (Heap) مُباشرة عن طريق تحديد مُخصص [`global_allocator`](https://github.com/solana-labs/solana/blob/8330123861a719cd7a79af0544617896e7f00ce3/sdk/program/src/entrypoint.rs#L50)

قد تقوم البرامج بتنفيذ `global_allocator` الخاصة بها بناءً على إحتياجاتها المُحددة. يرجى الرجوع إلى [custom heap example](#examples) للحصول على مزيد من المعلومات.

## القيود

تدعم برامج Rust على على الشبكة (on-chain) معظم libstd و libcore و liballoc من Rust، بالإضافة إلى العديد من صناديق الأطراف الخارجية.

هناك بعض القيود مُنذ أن تعمل هذه البرامج في بيئة مُقيدة بالموارد، ذات غطاء واحد، ويجب أن تكون حتمية: هناك بعض القيود نظرًا لأن هذه البرامج تعمل في بيئة محدودة الموارد (resource-constrained) وذات بيئة تشغيل وظيفة واحدة (single-threaded) ويجب أن تكون حتمية:

- لا يمكن الوصول إلى
  - `rand`
  - `std::fs`
  - `std::net`
  - `std::fs`
  - `std::future`
  - `std::net`
  - `std::process`
  - `std::sync`
  - `std::task`
  - `std::thread`
  - `std::time`
- وصول محدود إلى:
  - `std::hash`
  - `std::fs`
- يُعد رمز الـ Bincode مُكلفًا للغاية من الناحية الحسابية في كلتا الدورتين وعمق المُكالمة ويجب تجنبه
- يجب تجنب تنسيق السلسلة لأنها أيضا مُكلفة حسابيا.
- لا يوجد دعم لـ `println!` ،`print!`، يجب إستخدام [logging helpers](#logging) الخاصة بSolana بدلاً من ذلك.
- وقت التشغيل يفرض حداً على عدد التعليمات التي يمكن للبرنامج تنفيذها أثناء مُعالجة تعليمة واحدة. See [computation budget](developing/programming-model/runtime.md#compute-budget) for more information.

## إعتمادا على Rand

يتم تقييد البرامج للعمل بشكل حتمي، لذلك لا تتوفر أرقام عشوائية. في بعض الأحيان قد يعتمد البرنامج على صندوق يعتمد على نفسه `rand` حتى إذا كان البرنامج لا يستخدم أيًا من وظائف الأرقام العشوائية. إذا كان البرنامج يعتمد على `rand`، فستفشل عملية التجميع لأنه لا يوجد دعم `get-random` لـSolana. سيبدو الخطأ عادة مثل هذا:

```
error: target is not supported, for more information see: https://docs.rs/getrandom/#unsupported-targets
   --> /Users/jack/.cargo/registry/src/github.com-1ecc6299db9ec823/getrandom-0.1.14/src/lib.rs:257:9
    |
257 | /         compile_error!("\
258 | |             target is not supported, for more information see: \
259 | |             https://docs.rs/getrandom/#unsupported-targets\
260 | |         ");
    | |___________^
```

للعمل على حل مشكلة التبعية (dependency) هذه، أضف التبعية التالية إلى البرنامج `Cargo.toml`:

```
getrandom = { version = "0.1.14", features = ["dummy"] }
```

## التسجيل

الماكرو `println!` الخاص بRust مكلف حسابيًا وغير مدعوم. بدلاً من ذلك، يتم توفير الماكرو (macro) المُساعد [`msg!`](https://github.com/solana-labs/solana/blob/6705b5a98c076ac08f3991bb8a6f9fcb280bf51e/sdk/program/src/log.rs#L33).

`msg!` يحتوي على شكلين:

```rust
msg!("A string");
```

أو

```rust
msg!(0_64, 1_64, 2_64, 3_64, 4_64);
```

كلاهما يُخرجان النتائج إلى سجلات البرامج. إذا رغب أحد البرامج في ذلك، فيمكنه محاكاة `println!` بإستخدام `format!`:

```rust
msg!("Some variable: {:?}", variable);
```

يحتوي قسم [debugging](debugging.md#logging) على مزيد من المعلومات حول العمل مع سجلات البرنامج، ويحتوي [Rust examples](#examples) على مثال تسجيل.

## الهلع (Panicking)

تتم طباعة نتائج Rust آليا `panic!`, `assert!` ونتائج الذعر الداخلية إلى [program logs](debugging.md#logging).

```
INFO  solana_runtime::message_processor] Finalized account CGLhHSuWsp1gT4B7MY2KACqp9RUwQRhcUFfVSuxpSajZ
INFO  solana_runtime::message_processor] Call BPF program CGLhHSuWsp1gT4B7MY2KACqp9RUwQRhcUFfVSuxpSajZ
INFO  solana_runtime::message_processor] Program log: Panicked at: 'assertion failed: `(left == right)`
      left: `1`,
     right: `2`', rust/panic/src/lib.rs:22:5
INFO  solana_runtime::message_processor] BPF program consumed 5453 of 200000 units
INFO  solana_runtime::message_processor] BPF program CGLhHSuWsp1gT4B7MY2KACqp9RUwQRhcUFfVSuxpSajZ failed: BPF program panicked
```

### مُعالج الذعر المخصص (Custom Panic Handler)

يُمكن للبرامج أن تتجاوز مُعالج الذعر الإفتراضي عن طريق توفير تنفيذ خاص بها.

حَدّد أولاً أولا خاصيّة `custom-panic` في البرنامج `Cargo.toml`

```toml
[features]
default = ["custom-panic"]
custom-panic = []
```

ثم تقدم تطبيقا معمولا به لعامل الهلع (panic handler):

```rust
#[cfg(all(feature = "custom-panic", target_arch = "bpf"))]
#[no_mangle]
fn custom_panic(info: &core::panic::PanicInfo<'_>) {
    solana_program::msg!("program custom panic enabled");
    solana_program::msg!("{}", info);
}
```

في المقتطف أعلاه، يتم عرض التنفيذ الإفتراضي، لكن يمكن للمطورين إستبدال ذلك بشيء يناسب إحتياجاتهم بشكل أفضل.

أحد الآثار الجانبية لدعم رسائل الذعر الكاملة بشكل إفتراضي هو أن البرامج تتحمل تكلفة جذب المزيد من تنفيذ Rust `libstd` إلى الكائن المشترك للبرامج. ستسحب البرامج النموذجية بالفعل مبلغًا لا بأس به من `libstd` وقد لا تلاحظ زيادة كبيرة في حجم الكائن المشترك. لكن البرامج التي تحاول صراحة أن تكون صغيرة جدًا عن طريق تجنب `libstd` قد يكون لها تأثير كبير (حوالي 25kb). لإزالة هذا التأثير، يمكن للبرامج أن توفر مُعالج الذعر (panic handler) المُخصص الخاص بها بتنفيذ فارغ.

```rust
#[cfg(all(feature = "custom-panic", target_arch = "bpf"))]
#[no_mangle]
fn custom_panic(info: &core::panic::PanicInfo<'_>) {
    // Do nothing to save space
}
```

## حساب الميزانية (Compute Budget)

إستخدم إستدعاء النظام [`sol_log_compute_units()`](https://github.com/solana-labs/solana/blob/d3a3a7548c857f26ec2cb10e270da72d373020ec/sdk/program/src/log.rs#L102) لتسجيل رسالة تحتوي على العدد المتبقي من وحدات الحساب التي قد يستهلكها البرنامج قبل إيقاف التنفيذ

See [compute budget](developing/programming-model/runtime.md#compute-budget) for more information.

## تفريغ ELF أو ELF Dump

يمكن تفريغ العناصر الداخلية للكائن المشترك BPF في ملف نصي للحصول على نظرة ثاقبة أكثر في تكوين البرنامج وما يمكن أن يفعله في وقت التشغيل. سيحتوي التفريغ على معلومات ELF بالإضافة إلى قائمة بجميع الرموز والتعليمات التي تنفذها. بعض رسائل مُحمل BPF سوف تشير إلى أرقام تعليمات مُحددة حيث حدث الخطأ. يمكن البحث عن هذه الإشارات في ملفات تفريغ ELF لتحديد التعليمات المُسيئة وسياقها.

لإنشاء ملف تفريغ (dump):

```bash
$ cd <program directory>
$ cargo build-bpf --dump
```

## أمثلة

يحتوي المستودع [Solana Program Library github](https://github.com/solana-labs/solana-program-library/tree/master/examples/rust) على مجموعة من أمثلة اللغة البرمجية Rust.
