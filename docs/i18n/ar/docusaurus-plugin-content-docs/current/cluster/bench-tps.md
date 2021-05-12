---
title: تقييم مجموعة ما (Benchmark a Cluster)
---

يحتوي git Solana على جميع البرامج النَصِّية (scripts) التي قد تحتاج إليها للقيام بالإختبار الخاص بك في الشبكة التجريبية (testnet). إعتمادا على ما تتطلع إلى تحقيقه، قد ترغب في إجراء تغيير مُختلف، حيث أن الشبكة ذات العُقد (nodes) المُتعددة، المُتكاملة والمُحسنة الأداء أكثر تعقيدا من إعداد Rust-only، وعُقدة أُحادية (singlenode) عُقدة تجريبية (testnode). إذا كنت تبحث عن تطوير ميزات رفيعة المُستوى، مثل تجربة العقود الذكية (smart contracts)، وَفِّر على نفسك عناء الإعداد وإلتزم بعرض العُقدة الأُحادية التجريبية Rust-only. إذا كنت تقوم بتحسين أداء خط أنابيب (pipeline) المُعاملة، فلنضع في إعتبارك العرض التجريبي المُعَزَّز للعُقدة الأُحادية (singlenode). إذا كنت تقوم بعمل إجماع (consensus)، فستحتاج على الأقل إلى عرض Rust-only تجريبي مُتعدد العُقد (multinode). إذا كنت ترغب في إعادة إنتاج مقاييس المُعاملات في الثانية الواحدة (TPS) الخاصة بنا، قُم بتشغيل العرض التجريبي المُعَزَّز مُتعدد العُقد (multinode).

بالنسبة لجميع المُتغيرات الأربعة، ستحتاج إلى أحدث إصدار من سلسلة أدوات Rust وكود مصدر Solana:

أولا، إعداد Rust الحُمولة (Cargo) وحِزَم النظام (system packages) كما هو مُبين في ملف إقرأني الخاص بـ Solana [README](https://github.com/solana-labs/solana#1-install-rustc-cargo-and-rustfmt)

تحقق الآن من الكود من github:

```bash
git clone https://github.com/solana-labs/solana.git
cd solana
```

الكود التجريبي يتم فصله أحيانا بين الإصدارات وقتما نضيف يزات مُستوى مُنخفض جديدة، لذا إذا كانت هذه هي المرة الأولى التي تقوم فيها بتشغيل العرض التجريبي، ستحسن إحتمالات النجاح الخاصة بك إذا قُمت بالتحقق من أحدث إصدار [latest release](https://github.com/solana-labs/solana/releases) قبل المُتابعة:

```bash
TAG=$(git describe --tags $(git rev-list --tags --max-count=1))
git checkout $TAG
```

### تنصيب الإعدادات (Configuration Setup)

تأكد من أن البرامج المُهمة مثل برنامج التصويت (vote program) تم بناؤها قبل بدء أي عُقدة (node). لاحظ أننا نستخدم الإصدار الذي تم بناؤه هنا لتحقيق أداء جيد. إذا كنت ترغب في بناء التصحيح، إستخدم فقط حُمولة `cargo build`NDEBUG=1`NDEBUG=1` كجزء من الأمر.

```bash
cargo build --release
```

تتم تهيئة الشبكة بدفتر الأستاذ الذي تنشأ في مرحلة التكوين (genesis ledger) والذي تم إنشاؤه عن طريق تشغيل البرنامج النَصِّي (script) التالي.

```bash
NDEBUG=1 ./multinode-demo/setup.sh
```

### الصُّنبور (Faucet)

من أجل أن يعمل المُدقّقون (validators) والعملاء (clients)، سنحتاج إلى أن تُنشأ صنبورا (faucet) لإعطاء بعض الرموز التجريبية. يقدم الصُّنبور (Faucet) "توزيعات حرة" (air drops) على غرار نظام-ميلتون فريدمان (Milton Friedman) \(رموز مجانية للعملاء الذين يطلبونها\) لإستخدامها في مُعاملات الإختبار.

تشغيل الصُّنبور (Faucet) بواسطة الأمر البرمجي:

```bash
NDEBUG=1 ./multinode-demo/faucet.sh
```

### الشبكة التجريبية الأُحادية العُقدة (Singlenode Testnet)

قبل أن تبدأ تشغيل المُدقّق (validator)، تأكد من معرفة عنوان الIP الخاص بالآلة التي تريد أن تكون مُدقّق تمهيدي (bootstrap validator) للعرض التجريبي، وتأكد من أن منافذ (ports) الوصل udp ports 8000-10000 مفتوحة على جميع الآلات التي تريد إختبارها.

قُم الآن بتشغيل المُدقّق (bootstrap validator) في shell مُنفصلة:

```bash
NDEBUG=1 ./multinode-demo/bootstrap-validator.sh
```

إنتظر بضع ثوان لتهيئة الخادم. سيطبع "leader ready..." عندما يكون جاهزا لتلقي المُعاملات. سيطلب القائد (leader) بعض الرموز من الصُّنبور (Faucet) إذا لم يكن لديه أية رموز. ليس ضروريا أن يكون الصُّنبور (Faucet) شغالا حتى يشتغل القائد (leader) اللاحق.

### الشبكة التجريبية مُتعددة العُقد (Multinode Testnet)

لتشغيل الشبكة التجريبية مُتعددة العُقد (Multinode Testnet)، بعد تشغيل العُقدة القائد (leader node)، قُم بتدوير بعض المُدقّقين (validators) الإضافيين في shells مُنفصلة:

```bash
NDEBUG=1 ./multinode-demo/validator-x.sh
```

لتشغيل مُدقّق (validator) مُحسن الأداء على نظام التشغيل Linux، يجب تثبيت [CUDA 10.0](https://developer.nvidia.com/cuda-downloads) على النظام الخاص بك:

```bash
./fetch-perf-libs.sh
NDEBUG=1 SOLANA_CUDA=1 ./multinode-demo/bootstrap-validator.sh
NDEBUG=1 SOLANA_CUDA=1 ./multinode-demo/validator.sh
```

### إختبار تجريبي لعميل على الشبكة التجريبية (Testnet Client Dem)

الآن بعد أن تم تشغيل الشبكة التجريبية (testnet) لإختبارات العُقدة الأُحادية (singlenode) أو مُتعدد العُقد (multinode)، دعونا نُرسل لها بعض المُعاملات!

في shell مُنفصلة، قُم بتشغيل العميل (client):

```bash
NDEBUG=1 ./multinode-demo/bench-tps.sh # runs against localhost by default
```

ما الذي حدث للتو؟ يقوم العرض التجريبي للعميل بالدوران بعدة مُهمات مُتزامنة لإرسال 500000 مُعاملة إلى الشبكة التجريبية (testnet) بأسرع ما يُمكن. يقوم العميل (client) دوريا بتشغيل الشبكة التجريبية (testnet) لرؤية عدد المُعاملات التي تم القيام بها في ذلك الوقت. لاحظ أن العرض التجريبيي يغمر الشبكة عن قصد بحِزَم (packets) الـ UDP، بحيث من المُتَوَقَع تقريبا أن الشبكة سَتُسْقِط مجموعة منها. هذا يضمن أن الشبكة التجريبية (testnet) لها فرصة الوصول إلى 710 ألف مُعاملة في الثانية الواحدة (710k TPS). يكتمل العرض التجريبيي للعميل بعد أن يقتنع أن الشبكة التجريبية (testnet) لن تُعالج أي مُعاملات إضافية. يجب أن ترى عدة قياسات مُعاملات في الثانية الواحدة (TPS) مطبوعة على الشاشة. في مُتغيرة المُتَعَدِّدَ العُقد (multinode)، سترى كذلك قياسات مُعاملات في الثانية الواحدة (TPS) لكل عُقدة تدقيق (valdiator node).

### تصحيح أخطاء الشبكة التجريبية (Testnet Debugging)

هناك بعض رسائل التصحيح (debug messages) المُفيدة في الكود، يُمكنك تفعيلها على أساس كل وحدة وكل مُستوى. قبل تشغيل القائد (leader) أو المُدقّق (validator) قُم بتعيين مُتغيرة بيئة RUST_LOG العادية.

على سبيل المثال

- لتفعيل المعلومات `info` في كل مكان وتصحيح الأخطاء `debug` فقط في ملف solana::banking_stage:

  ```bash
export RUST_LOG=solana=info,solana::banking_stage=debug
  ```

- لتفعيلها سِجَل برنامج BPF:

  ```bash
export RUST_LOG=solana_bpf_loader=trace
  ```

بشكل عام، نستخدم تصحيح الأخطاء `debug` لرسائل تصحيح الأخطاء غير المُتكررة، وتتبع `trace` الرسائل التي يحتمل أن تكون مُتكررة ومعلومات `info` التسجيل المُرتبط بالأداء.

يُمكنك أيضا إرفاق عملية قيد التشغيل مع GDB. عملية القائد (leader) تُسمى _solana-validator_:

```bash
sudo gdb
attach <PID>
set logging on
thread apply all bt
```

سيُؤدي هذا إلى تدوين جميع آثار العمليات المُتزامنة في gdb.txt

## شبكة المُطوِّر التجريبية (Developer Testnet)

في هذا المثال يتصل العميل (client) بالشبكة التجريبية العامة (public testnet) الخاصة بنا. لتشغيل المُدقّقين (validators) على الشبكة التجريبية الشبكة التجريبية (tesnet) ستحتاج إلى فتح المنافذ (ports) التالية udp ports `8000-10000`.

```bash
NDEBUG=1 ./multinode-demo/bench-tps.sh --entrypoint devnet.solana.com:8001 --faucet devnet.solana.com:9900 --duration 60 --tx_count 50
```

يُمكنك مُلاحظة تأثيرات مُعاملات العميل (client) الخاص بك على لوحة تحكم القياسات [metrics dashboard](https://metrics.solana.com:3000/d/monitor/cluster-telemetry?var-testnet=devnet)
