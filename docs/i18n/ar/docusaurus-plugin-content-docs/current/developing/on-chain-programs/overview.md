---
title: "نظرة عامة"
---

يمكن للمطورين كتابة ونشر برامجهم الخاصة على شبكة بلوكشاين Solana.

يُعد [Helloworld example](examples.md#helloworld) نقطة إنطلاق جيدة لمعرفة كيفية كتابة البرنامج وبنائه ونشره والتفاعل معه عبر الشبكة (on-chain).

## مُرشح حزمة بيركلي (BPF) أو Berkley Packet Filter

يتم تجميع برامج Solana عبر الشبكة (on-chain) عبر [LLVM compiler infrastructure](https://llvm.org/) إلى [Executable and Linkable Format (ELF)](https://en.wikipedia.org/wiki/Executable_and_Linkable_Format) تحتوي على تباين من bytecode ال[Berkley Packet Filter (BPF)](https://en.wikipedia.org/wiki/Berkeley_Packet_Filter).

نظرًا لأن Solana تستخدم البنية التحتية للمحول البرمجي LLVM، فقد تتم كتابة البرنامج بأي لغة برمجة يمكنها إستهداف الواجهة الخلفية لـ BPF الخاص بـ LLVM. تدعم Solana حاليا برامج الكتابة باللغة البرمجية Rust و C/C+++.

يوفر BPF مجموعة تعليمات فعالة [instruction set](https://github.com/iovisor/bpf-docs/blob/master/eBPF.md) يمكن تنفيذها بواسطة آلة افتراضية مترجمة أو كإرشادات أصلية مُجمعة وفعالة في الوقت المناسب.

## خريطة الذاكرة (Memory map)

تم إصلاح خريطة ذاكرة العنوان الظاهرية التي تستخدمها برامج BPF الخاصة بـ Solana وتم وضعها على النحو التالي

- رمز البرنامج يبدأ عند 0x1000000
- بيانات المُكدس (Stack) تبدأ عند 0x200000000
- بيانات كومة الذاكرة المؤقتة (Heap) تبدأ عند 0x300000000
- مُعلمات (parameters) مُدخل (input) البرنامج تبدأ عند 0x400000000

العناوين الإفتراضية المذكورة أعلاه هي عناوين البداية ولكن يتم منح البرامج وصولاً إلى مجموعة فرعية من خريطة الذاكرة. سيُصاب البرنامج بالذعر إذا حاول القراءة أو الكتابة إلى عنوان إفتراضي لم يتم منحه حق الوصول إليه، وسيتم إرجاع خطأ `AccessViolation` يحتوي على عنوان وحجم محاولة الإنتهاك.

## المُكدس (Stack)

يستخدم BPF أُطر تكدس بدلاً من مُؤشر تكدس مُتغير. كل إطار مُكدس بحجم 4KB.

إذا كان البرنامج ينتهك حجم إطار التكدس، فسيبلغ المُحول البرمجي (compiler) عن التجاوز كتحذير.

For example: `Error: Function _ZN16curve25519_dalek7edwards21EdwardsBasepointTable6create17h178b3d2411f7f082E Stack offset of -30728 exceeded max offset of -4096 by 26632 bytes, please minimize large stack variables`

تحدد الرسالة الرمز الذي يتجاوز إطاره المُكدس الخاص به ولكن الإسم قد يكون مشوهًا إذا كان رمزا لـ Rust أو C++. لإزالة التشوه من رمز Rust إستخدم [rustfilt](https://github.com/luser/rustfilt). التحذير أعلاه جاء من برنامج Rust، لذلك إسم الرمز المُشوه هو:

```bash
$ rustfilt _ZN16curve25519_dalek7edwards21EdwardsBasepointTable6create17h178b3d2411f7f082E
curve25519_dalek::edwards::EdwardsBasepointTable::create
```

لإزالة تشوه رمز C++ إستخدم `c+++filt` من binutils.

السبب في الإبلاغ عن تحذير بدلاً من الخطأ هو أن بعض الصناديق التابعة قد تتضمن وظائف تنتهك قيود إطار المُكدس حتى إذا كان البرنامج لا يستخدم هذه الوظيفة. إذا كان البرنامج ينتهك حجم المُكدس في وقت التشغيل، سيتم الإبلاغ عن `AccessViolation` خطأ.

تشغل أُطر تكدس BPF نطاق عنوان إفتراضي يبدأ من 0x200000000.

## عمق الإتصال (Call Depth)

يتم تقييد البرامج للعمل بسرعة، ولتسهيل ذلك، فإن مُكدس إستدعاء البرنامج يقتصر على عمق 64 إطارًا (frames) كحد أقصى.

## كومة الذاكرة المُؤقتة (Heap)

البرامج لديها إمكانية الوصول إلى فترة تشغيل إما مباشرة في C أو عبر واجهة برمجة التطبيقات (API) الخاصة بـ Rust `alloc`. لتسهيل عمليات التخصيص السريعة، يتم إستخدام كومة ذاكرة مُؤقتة (Heap) بسيطة من 32KB. كومة الذاكرة المُؤقتة (Heap) لا تدعم `free` أو `realloc` لذلك إستخدمها بحكمة.

داخليًا، يمكن للبرامج الوصول إلى منطقة الذاكرة 32KB بدءًا من العنوان الإفتراضي 0x300000000 ويمكنها تنفيذ كومة ذاكرة مُؤقتة (Heap) مُخصصة بناءً على الإحتياجات المُحددة للبرنامج.

- [إستخدام برنامج Rust لكومة الذاكرة المُؤقتة (Heap)](developing-rust.md#heap)
- [إستخدام برنامج C لكومة الذاكرة المُؤقتة (Heap)](developing-c.md#heap)

## الدعم العائم (Float Support)

Programs support a limited subset of Rust's float operations, if a program attempts to use a float operation that is not supported, the runtime will report an unresolved symbol error.

Float operations are performed via software libraries, specifically LLVM's float builtins. Due to be software emulated they consume more compute units than integer operations. In general, fixed point operations are recommended where possible.

The Solana Program Library math tests will report the performance of some math operations: https://github.com/solana-labs/solana-program-library/tree/master/libraries/math

To run the test, sync the repo, and run:

`$ cargo test-bpf -- --nocapture --test-threads=1`

Recent results show the float operations take more instructions compared to integers equivalents. Fixed point implementations may vary but will also be less then the float equivalents:

```
         u64   f32
Multipy    8   176
Divide     9   219
```

## البيانات القابلة للكتابة الثابتة (Static Writable Data)

الكائنات المُشتركة للبرنامج لا تدعم البيانات المُشتركة القابلة للكتابة. تتم مُشاركة البرامج بين عمليات التنفيذ المُتوازية المُتعددة بإستخدام نفس التعليمات البرمجية والبيانات المشتركة للقراءة فقط. هذا يعني أنه لا ينبغي للمُطورين تضمين أي مُتغيرات كتابية ثابتة أو عالمية في البرامج. يُمكن في المُستقبل إضافة آلية نسخ عند الكتابة لدعم البيانات القابلة للكتابة.

## القسم المُوقّع (Signed division)

مجموعة تعليمات BPF لا تدعم [signed division](https://www.kernel.org/doc/html/latest/bpf/bpf_design_QA.html#q-why-there-is-no-bpf-sdiv-for-signed-divide-operation). أخذنا في الإعتبار إضافة تعليمات قسم مُوقّع.

## المُحمّلون (Loaders)

يتم نشر وتنفيذه البرامج مع برامج تحميل وقت التشغيل، يوجد حاليًا مُحمّلان (Loaders) مدعومان [BPF Loader](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/bpf_loader.rs#L17) و [BPF loader deprecated](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/bpf_loader_deprecated.rs#L14)

قد تدعم برامج التحميل واجهة تطبيق ثنائية مختلفة، حيث يجب على المطورين كتابة برامجهم ونشرها على نفس المُحمّل (Loader). إذا تم نشر برنامج مكتوب لمُحمّل (Loader) واحد على برنامج آخر، فعادة ما تكون النتيجة خطأ `AccessViolation` بسبب عدم تطابق إلغاء تسلسل إدخال مُعلمات البرنامج.

لجميع الأغراض العملية يجب أن يكتب البرنامج دائما لاستهداف أحدث محمل BPF وآخر محمل هو الافتراضي لواجهة سطر الأوامر و API جافا سكريبت. لجميع الأغراض العملية، يجب دائمًا كتابة البرنامج لإستهداف أحدث مُحمّل BPF وأحدث مُحمّل هو واجهة سطر الأوامر (command-line interface) وواجهات برمجة تطبيقات (API) جافاسكريبت.

للحصول على معلومات خاصة بلغة معينة حول تنفيذ برنامج لمُحمّل (Loader) مُعين، راجع:

- [نقاط دخول (entrypoints) برنامج Rust](developing-rust.md#program-entrypoint)
- [نقاط دخول (entrypoints) برنامج C](developing-c.md#program-entrypoint)

### النشر (Deployment)

نشر برنامج BPF هو عملية تحميل كائن BPF مُشترك في بيانات حساب البرنامج ووضع علامة على الحساب القابل للتنفيذ. يقوم العميل بتقسيم الكائن المُشترك BPF إلى أجزاء أصغر ويُرسلها كبيانات تعليمات من [`Write`](https://github.com/solana-labs/solana/blob/bc7133d7526a041d1aaee807b80922baa89b6f90/sdk/program/src/loader_instruction.rs#L13) تعليمات إلى المُحمِّل (loader) حيث يكتب المُحمِّل تلك البيانات في بيانات حساب البرنامج. بمُجرد إستلام جميع القطع، يُرسل العميل تعليمات [`Finalize`](https://github.com/solana-labs/solana/blob/bc7133d7526a041d1aaee807b80922baa89b6f90/sdk/program/src/loader_instruction.rs#L30) إلى المُحمِّل، ثم يتحقق المُحمِّل من صحة بيانات BPF ويضع علامة على حساب البرنامج على أنه _executable_. بمجرد وضع علامة على حساب البرنامج على أنه قابل للتنفيذ، يُمكن للمُعاملات اللاحقة إصدار تعليمات لذلك البرنامج للمُعالجة.

عندما يتم توجيه تعليمات إلى برنامج BPF قابل للتنفيذ، يقوم المُحمّل بتهيئة بيئة تنفيذ البرنامج، تسلسل مُعلمات إدخال البرنامج، إستدعاء نقطة دخول البرنامج والإبلاغ عن أي أخطاء تمت مُواجهتها.

للحصول على مزيد من المعلومات، راجع [deploying](deploying.md)

### تسلسل مُعلمة الإدخال (Input Parameter Serialization)

تقوم برامج تحميل BPF بتسلسل مُعلمات إدخال البرنامج في مصفوفة byte يتم تمريرها بعد ذلك إلى نقطة دخول البرنامج، حيث يكون البرنامج مسؤولاً عن إلغاء تسلسلها على على الشبكة (on-chain). تتمثل أحد التغييرات بين المُحمّل المُهمل والمُحمّل الحالي في أن مُعلِّمات الإدخال يتم تسلسلها بطريقة تُؤدي إلى وقوع مُعلِّمات مختلفة في إزاحات مُتحاذية داخل مصفوفة الbyte المحاذاة. يسمح هذا لعمليات إلغاء التسلسل بالإشارة مُباشرة إلى مصفوفة الbyte وتوفير مُؤشرات مُتوائمة للبرنامج.

للحصول على معلومات خاصة بلغة معينة حول التسلسل، راجع:

- [إزالة تسلسل مُعلِّمة برنامج Rust](developing-rust.md#parameter-deserialization)
- [إزالة تسلسل مُعلِّمة برنامج C](developing-c.md#parameter-deserialization)

أحدث تسلسل لمُعلِّمات إدخال البرنامج على النحو التالي (كل الترميز هو قليل من الترميز):

- 8 byte عدد حسابات غير موقعة
- لكل حساب
  - عدد 1 بايت يُشير إلى ما إذا كان هذا حسابا مُكررًا، إن لم يكن مُكررًا ، تكون القيمة 0xff، وإلا فإن القيمة هي فهرس الحساب الذي تكون نسخة مُكررة منه.
  - عدد 7 byte من الحشوة
    - إن لم يكن مكرراً
      - عدد 1 byte من الحشوة
      - عدد 1 byte المنطقية (boolean)، صحيح إذا كان الحساب مُوَقِّع (signer)
      - عدد 1 byte المنطقية (boolean)، صحيح إذا كان الحساب قابل للكتابة
      - عدد 1 byte المنطقية (boolean)، صحيح إذا كان الحساب قابل للتنفيذ
      - عدد 4 byte من الحشوة
      - عدد 32 byte للمفتاح العمومي (public key) للحساب
      - عدد 32 byte للمفتاح العمومي (public key) لمالك الحساب
      - عدد 8 byte من الlamports الغير مُوَقِّعة التي يملكها الحساب
      - عدد 8 byte من عدد غير مُوَقِّع من بيانات الحساب
      - عدد x byte من بيانات الحساب
      - عدد 10k byte من الحشوة (padding)، يستخدم للrealloc
      - ما يكفي من الحشوة لمُواءمة الإزاحة مع 8 bytes.
      - عدد 8 byte إيجار للفترة (Epoch)
- عدد 8 byte عدد غير مُوقّع لبيانات التعليمات
- عدد x byte عدد غير مُوقّع لبيانات التعليمات
- عدد 32 byte بايت لمعرف البرنامج
