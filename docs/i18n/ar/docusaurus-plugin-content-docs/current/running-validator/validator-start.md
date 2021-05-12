---
title: البدأ في تشغيل المُدقّق (Starting a Validator)
---

## إعداد CLI Solana

تتضمن أداة solana cli أوامر إعدادات `get` و `set` لتعيين وسيطة `--url` تلقائيًا لأوامر cli. على سبيل المثال:

```bash
مجموعة إعدادات solana --url http://devnet.solana.com
```

بينما يُوضح هذا القسم كيفية الإتصال بمجموعة شبكة المُطورين (Devnet)، فإن الخطوات مُماثلة لخطوات [Solana Clusters](../clusters.md) الأخرى.

## تأكد من إمكانية الوصول إلى المجموعة (cluster)

قبل إرفاق عُقد التدقيق (validator nodes)، تحقق من سلامة الوصول إلى المجموعة من جهازك عن طريق جلب عدد المُعاملات:

```bash
solana transaction-count
```

إطلع على لوحة القياسات [metrics dashboard](https://metrics.solana.com:3000/d/monitor/cluster-telemetry) لمزيد من التفاصيل حول نشاط المجموعة (cluster).

## قم بتأكيد التثبيت الخاص بك (Confirm your Installation)

حاول تشغيل الأمر التالي للإنضمام إلى شبكة القيل والقال (gossip) وعرض جميع العُقد (nodes) الأخرى في المجموعة (cluster):

```bash
solana-gossip spy --entrypoint devnet.solana.com:8001
# Press ^C to exit
```

## تفعيل CUDA

إذا كان جهازك يحتوي على وحدة مُعالجة الرسومات (GPU) مُثبت عليه \(Linux-only حاليًا\)، فقُم بتضمين وسيطة `--cuda` إلى `solana-validator`.

عند بدأ تشغيل المُدقّق (validator)، إبحث عن رسالة السجل التالية للإشارة إلى تفعيل `"[<timestamp> solana::validator] CUDA is enabled"`

## ضبط النظام (System Tuning)

### نظام التشغيل لينكس (Linux)
#### أوتوماتيكي (Automatic)
يتضمن solana repo برنامجًا خفيًا لضبط إعدادات النظام لتحسين الأداء (أي عن طريق زيادة المخزن المُؤقت (buffer) لنظام التشغيل UDP وحدود تعيين الملفات).

تم تضمين الـ daemon أو البرنامج الخفي (`solana-sys-tuner`) في إصدار solana الثنائي (solana binary release). أعد تشغيله، قبل *before* إعادة تشغيل المُدقّق (validator) الخاص بك، بعد كل ترقية للبرنامج للتأكد من تطبيق أحدث الإعدادات المُوصى بها.

لتشغيله:

```bash
sudo solana-sys-tuner --user $(whoami) > sys-tuner.log 2>&1 &
```

#### الدليل (Manual)
إذا كنت تفضل إدارة إعدادات النظام بنفسك، فيُمكنك القيام بذلك بإستخدام الأوامر (commands) التالية.

##### **زيادة المخزن المُؤقت (buffer) لـ UDP**
```bash
sudo bash -c "cat >/etc/sysctl.d/20-solana-udp-buffers.conf <<EOF
# Increase UDP buffer size
net.core.rmem_default = 134217728
net.core.rmem_max = 134217728
net.core.wmem_default = 134217728
net.core.wmem_max = 134217728
EOF"
```
```bash
sudo sysctl -p /etc/sysctl.d/20-solana-udp-buffers.conf
```

##### **زيادة حدود ملفات الذاكرة المُخططة (Increased memory mapped files limit)**
```bash
sudo bash -c "cat >/etc/sysctl.d/20-solana-mmaps.conf <<EOF
# Increase memory mapped files limit
vm.max_map_count = 500000
EOF"
```
```bash
sudo sysctl -p /etc/sysctl.d/20-solana-mmaps.conf
```
إضافة
```
LimitNOFILE=500000
```
إلى قسم `[Service]` من ملف خدمة systemd الخاص بك، إذا كنت تستخدم واحدًا، فقُم بإضافة
```
DefaultLimitNOFILE=500000
```
إلى قسم `[Manager]` من `/etc/systemd/system.conf`.
```bash
sudo systemctl daemon-reload
```
```bash
sudo bash -c "cat >/etc/security/limits.d/90-solana-nofiles.conf <<EOF
# Increase process file descriptor count limit
* - nofile 500000
EOF"
```
```bash
### Close all open sessions (log out then, in again) ###
```

## توليد الهوية (Generate identity)

قم بإنشاء زوج مفاتيح هوية (identity keypair) للمُدقّق (validator) الخاص بك من خلال تشغيل:

```bash
solana-keygen new -o ~/validator-keypair.json
```

يُمكن الآن عرض مفتاح الهوية العام (identity public key) عن طريق تشغيل:

```bash
solana-keygen pubkey ~/validator-keypair.json
```

> ملاحظة: ملف "validator-keypair.json" هو أيضًا مفتاحك الخاص \ (ed25519\).

### هوية المحفظة الورقية (Paper Wallet identity)

يُمكنك إنشاء محفظة ورقية (Paper wallet) لملف هويتك بدلاً من كتابة ملف زوج مفاتيح (keypair) على القرص بإستخدام:

```bash
solana-keygen new --no-outfile
```

يُمكن الآن عرض المفتاح العمومي (public key) المُطابق عن طريق تشغيل:

```bash
solana-keygen pubkey ASK
```

ثم إدخال كلمات الإسترداد (seed phrase) الخاصة بك.

راجع إستخدام المحفظة الورقية [Paper Wallet Usage](../wallet-guide/paper-wallet.md) لمزيد من المعلومات.

---

### زوج المفاتيح الذاتي (Vanity Keypair)

يُمكنك إنشاء زوج مفاتيح ذاتي (Vanity Keypair) مُخصص بإستخدام solana-keygen. على سبيل المثال:

```bash
solana-keygen grind --starts-with e1v1s:1
```

إعتمادًا على السلسلة المطلوبة، قد يستغرق الأمر أيامًا للعثور على تطابق...

---

يُعرّف زوج زوج مفاتيح الهوية (identity keypair) المُدقّق (validator) الخاص بك بشكل فريد داخل الشبكة. من الأهمية بمكان إجراء نسخ إحتياطي لهذه المعلومات **It is crucial to back-up this information.**

إذا لم تقم بعمل نسخة إحتياطية من هذه المعلومات، فلن تكون قادرًا على إسترداد المُدقّق (validator) الخاص بك إذا فقدت الوصول إليها. إذا حدث هذا، فستفقد حِصَّتك من عملات SOL أيضًا.

لعمل نسخة إحتياطية من زوج مفاتيح الهوية (identity keypair) للمُدقّق (validator) الخاص بك، قُم بعمل نسخة إحتياطية من ملفك أو كلمات الإسترداد (seed phrase) في مكان آمن **back-up your "validator-keypair.json” file or your seed phrase to a secure location.**

## المزيد من إعدادات Solana CLI

الآن بعد أن أصبح لديك زوج مفاتيح (keypair)، قُم بضبط إعدادات solana لاستخدام زوج مفاتيح (keypair) المُدقّق (validator) الخاص بك لجميع الأوامر (commands) التالية:

```bash
solana config set --keypair ~/validator-keypair.json
```

يجب أن ترى الناتج التالي:

```text
Wallet Config Updated: /home/solana/.config/solana/wallet/config.yml
* url: http://devnet.solana.com
* keypair: /home/solana/validator-keypair.json
```

## توزيع حر & تحقق من رصيد المُدقّق (validator) الخاص بك

أرسل لنفسك (Airdrop) بعضًا من عملات SOL للبدأ:

```bash
توزيع حر (airdrop) لـ Solana لعدد 10 من عملة Sol
```

خُذ في الإعتبار أن أن التوزيع الحر (Airdrop) مُتاح فقط على شبكة المُطورين (Devnet) و الشبكة التجريبية (testnet). كلاهما مُقيد بعدد 10 من عملة SOL لكل طلب.

لعرض رصيدك الحالي:

```text
solana balance
```

أو لرؤية بتفاصيل أدق:

```text
solana balance --lamports
```

إقرأ المزيد عن الـ الفرق بين SOL و lamports هنا [difference between SOL and lamports here](../introduction.md#what-are-sols).

## إنشاء حساب تصويت (Create Vote Account)

إذا لم تكن قد قُمت بذلك بالفعل، فأنشئ زوج مفاتيح حساب التصويت (vote-account keypair) وأنشئ حساب تصويت على الشبكة. إذا كنت قد أكملت هذه الخطوة، يجب أن ترى "voice-account-keypair.json" في دليل وقت تشغيل Solana:

```bash
solana-keygen new -o ~/vote-account-keypair.json
```

يُمكن إستخدام الأمر التالي لإنشاء حساب تصويت على البلوكشاين مع جميع الخيارات الإفتراضية:

```bash
solana create-vote-account ~/vote-account-keypair.json ~/validator-keypair.json
```

إقرأ المزيد عن إنشاء وإدارة حساب تصويت [creating and managing a vote account](vote-accounts.md).

## المُدقّقون الموثقون (Trusted validators)

إذا كنت تعرف وتثق في عُقد المُدقّقين (validator nodes) الأخرى، يُمكنك تحديد ذلك في سطر الأوامر (command line) بإستخدام الرقم `--trusted-validator <PUBKEY>` argument to `solana-validator`. يُمكنك تحديد العديد بتكرار الوسيطة `--trusted-validator <PUBKEY1> --trusted-validator <PUBKEY2>`. هذا له تأثيران، أحدهما عندما يقوم المُدقّق (validator) ببدأ الإشتغال أو التمهيد (booting) بإستخدام `--no-untrusted-rpc`، سيطلب فقط تلك المجموعة من العُقد (nodes) الموثوقة لتنزيل بيانات مرحلة التكوين (Genesis) واللقطة (snapshot). جزئية أخرى وهي أنه بالإقتران مع الخيار `--halt-on-trusted-validator-hash-mismatch`، فإنه سيُراقب تجزئة جذر Merkle لحالة الحسابات الكاملة للعُقد (nodes) الموثوقة الأخرى على القيل والقال (gossip) وإذا كانت التجزئات تنتج أي عدم تطابق، فسيقوم المُدقّق (validator) بإيقاف العُقدة (node) لمنع المُدقّق من التصويت أو مُعالجة قِيم حالة (state values) يُحتمل أن تكون غير صحيحة. في الوقت الحالي، فإن الفُتحة (Slot) التي ينشر المُدقّق (validator) التجزئة عليها مُرتبطة بفاصل اللقطة (snapshot interval). لكي تكون الميزة فعالة، يجب تعيين جميع المُدقّقين (validators) في المجموعة الموثوقة على نفس قيمة الفاصل الزمني للقطة (snapshot) أو مُضاعفاتها.

يُوصى بشدة بإستخدام هذه الخيارات لمنع تنزيل حالة اللقطة (snapshot) الخبيثة أو تباعد حالة الحساب.

## قُم ببتوصيل المُدقّق (validator) الخاص بك

قُم بالإتصال بالمجموعة (cluster) عن طريق تشغيل الأمر البرمجي:

```bash
solana-validator \
  --identity ~/validator-keypair.json \
  --vote-account ~/vote-account-keypair.json \
  --ledger ~/validator-ledger \
  --rpc-port 8899 \
  --entrypoint devnet.solana.com:8001 \
  --limit-ledger-size \
  --log ~/solana-validator.log
```

لفرض تسجيل المُدقّق (validator) إلى وحدة التحكم، أضف وسيطة `--log -`، وإلا فسيقوم المُدقّق (validator) بتسجيل الدخول تلقائيًا إلى ملف.

> ملاحظة: يمكنك إستخدام ملف كلمات إستِرداد المحفظة الورقية [paper wallet seed phrase](../wallet-guide/paper-wallet.md) for your `--identity` and/or `--authorized-voter` keypairs. لإستخدامها، قُم بتمرير الوسيطة المُعنية كـ  `solana-validator --identity ASK ... --authorized-voter ASK ...` وسيُطلب منك كلمات الإستِرداد وكلمة المرور الإختيارية.

قُم بتأكيد إتصال المُدقّق (validator) الخاص بك بالشبكة عن طريق فتح محطة طرفية جديدة وتشغيل:

```bash
solana-gossip spy --entrypoint devnet.solana.com:8001
```

إذا كان المُدقّق (validator) الخاص بك مُتصلاً، فسيظهر مفتاحه العمومي (public key) وعنوان الـ IP الخاص به في القائمة.

### التحكم في تخصيص منفذ (port) الشبكة المحلية

بشكل إفتراضي، سيُحدد المُدقّق (validator) ديناميكيًا منافذ (ports) الشبكة المُتاحة في النطاق 8000-10000، ويُمكن تجاوزها بـ ` --dynamic-port-range `. على سبيل المثال، سوف يُقيِّد `solana-validator --dynamic-port-range 11000-11010 ...` المُدقّق (validator) على المنافذ 11000-11010.

### تحديد حجم دفتر الأستاذ (ledger) للحفاظ على مساحة القرص
تسمح لك المُعلمة `--limit-ledger-size` بتحديد عدد قِطَع [shreds](../terminology.md#shred) دفتر الأستاذ الذي تحتفظ به العُقدة (node) على القرص. إذا لم تقم بتضمين هذه المُعلِّمة (parameter)، فسوف يحتفظ المُدقّق (validator) بدفتر الأستاذ (ledger) بالكامل حتى نفاد مساحة القرص.

تُحاول القيمة الافتراضية إبقاء إستخدام قرص دفتر الأستاذ (ledger) أقل من 500GB.  قد يتم طلب إستخدام أكثر أو أقل للقرص عن طريق إضافة وسيطة إلى `--limit-ledger-size` إذا رغبت في ذلك. تحقق من `solana-validator --help` لقيمة الحد الإفتراضية المُستخدمة بواسطة `--limit-ledger-size`.  مزيد من المعلومات حول إختيار قيمة حد مُخصصة هي موجودة هنا [available here](https://github.com/solana-labs/solana/blob/583cec922b6107e0f85c7e14cb5e642bc7dfb340/core/src/ledger_cleanup_service.rs#L15-L26).

### وحدة النظام (Systemd Unit)
يعد تشغيل المُدقّق (validator) كوحدة systemd هي طريقة سهلة لإدارة التشغيل في الخلفية.

على افتراض أن لديك مستخدم يسمى `sol` على جهازك، قم بإنشاء الملف `/etc/systemd/system/sol.service` من خلال ما يلي:
```
[Unit]
Description=Solana Validator
After=network.target
Wants=solana-sys-tuner.service
StartLimitIntervalSec=0

[Service]
Type=simple
Restart=always
RestartSec=1
User=sol
LimitNOFILE=500000
LogRateLimitIntervalSec=0
Environment="PATH=/bin:/usr/bin:/home/sol/.local/share/solana/install/active_release/bin"
ExecStart=/home/sol/bin/validator.sh

[Install]
WantedBy=multi-user.target
```

الآن قُم بإنشاء `/home/sol/bin/validator.sh` لتضمين سطر الأوامر `solana-validator` المطلوب.  تأكد من أن تشغيل `/home/sol/bin/validator.sh` يبدأ المُدقّق (validator) يدويًا كما هو مُتوقع. لا تنسى وضع علامة قابل للتنفيذ (executable) بـ `chmod +x /home/sol/bin/validator.sh`

إبدأ الخدمة بـ:
```bash
$ sudo systemctl enable --now sol
```

### الضبط (Logging)
#### ضبط إخراج السجل (Log output tuning)

يُمكن التحكم في الرسائل التي يُرسلها المُدقّق (validator) إلى السجل بواسطة مُتغير البيئة `RUST_LOG`. يُمكن العثور على التفاصيل في [documentation](https://docs.rs/env_logger/latest/env_logger/#enabling-logging) لـ `env_logger` صندوق Rust.

لاحظ أنه إذا تم تقليل مُخرجات الضبط (logging output)، فقد يُؤدي ذلك إلى صعوبة تصحيح المُشكلات التي ستتم مُواجهتها لاحقًا. في حالة طلب الدعم من الفريق، يجب التراجع عن أي تغييرات وإعادة إنتاج المُشكلة قبل تقديم المُساعدة.

#### تدوير السجل (Log rotation)

يُمكن أن يصبح ملف سجل المُدقّق (validator)، كما هو مُحدد بواسطة `--log ~/solana-validator.log`، كبيرًا جدًا بمرور الوقت ويُوصى بتكوين تدوير السجل.

سيُعيد المُدقّق (validator) فتحه عندما يتلقى إشارة `USR1`، وهي الإشارة الأساسية التي تُتيح تدوير السجل (log rotation).

#### بإستخدام تدوير السجل (Using logrotate)

مثال على إعداد `logrotate`، والذي يفترض أن المُدقّق (validator) يعمل كخدمة systemd تُسمى `sol.service` ويكتب ملف سجل على /home/sol/solana-validator.log:
```bash
# Setup log rotation

cat > logrotate.sol <<EOF
/home/sol/solana-validator.log {
  rotate 7
  daily
  missingok
  postrotate
    systemctl kill -s USR1 sol.service
  endscript
}
EOF
sudo cp logrotate.sol /etc/logrotate.d/sol
systemctl restart logrotate.service
```

### تعطيل عمليات فحص المنفذ (port) لتسريع عمليات إعادة التشغيل
بمجرد أن يعمل المُدقّق (validator) بشكل طبيعي، يُمكنك تقليل الوقت المُستغرق لإعادة تشغيل المُدقّق (validator) عن طريق إضافة علامة `--no-port-check` إلى سطر أوامر `solana-validator`.

### تعطيل ضغط اللقطة (snapshot) لتقليل إستخدام وحدة المُعالجة المركزية (CPU)
إذا كنت لا تقدم اللقطات (snapshots) إلى جهات المُدقّقين (validators) الأخرى، فيُمكن تعطيل ضغط اللقطة (snapshot) لتقليل حِمل وحدة المُعالجة المركزية (CPU) على حساب إستخدام القرص بشكل أكبر قليلاً لتخزين اللقطات (snapshots) المحلية.

أضف الوسيطة `--snapshot-compression none` إلى وسيطات سطر الأوامر `solana-validator` وأعد تشغيل المُدقّق (validator).

### إستخدام ramdisk مع الإمتداد والإنتشار إلى المُبادلة (spill-over into swap) لقاعدة بيانات الحسابات لتقليل تآكل أقراص الحالة الصلبة (SSD)
إذا كان جهازك يحتوي على الكثير من ذاكرة الوصول العشوائ (RAM)، فيُمكن إستخدام tmpfs ramdisk ([tmpfs](https://man7.org/linux/man-pages/man5/tmpfs.5.html)) للإحتفاظ بقاعدة بيانات الحسابات

عند إستخدام tmpfs، من الضروري أيضًا تكوين المُبادلة على جهازك أيضًا لتجنب نفاد مساحة tmpfs بشكل دوري.

يُوصى بإستخدام قسم tmpfs بسِعة 300GB، مع قسم مُبادلة مُصاحب بسِعة 250GB.

مثال الإعدادات (Example configuration):
1. `sudo mkdir /mnt/solana-accounts`
2. أضف قسمًا بحجم 300GB لـ tmpfs عن طريق إضافة سطر جديد يحتوي على `tmpfs
/mnt/solana-accounts tmpfs rw,size=300G,user=sol 0 0` إلى `/etc/fstab` (على إفتراض أن المُدقّق الخاص بك يعمل تحت المُستخدم "sol").  **CAREFUL: If you incorrectly edit /etc/fstab your machine may no longer boot**
3. قم بإنشاء 250GB على الأقل من مساحة التبديل
  - إختر جهازًا لإستخدامه بدلاً من `SWAPDEV` لبقية هذه التعليمات. من الناحية المثالية، حدد قسمًا مجانيًا للقرص بحجم 250GB أو أكبر على قرص سريع. إذا لم يكن أحدهما مُتاحًا، فقُم بإنشاء ملف مُبادلة بـ `sudo dd if=/dev/zero of=/swapfile bs=1MiB count=250KiB`، وقُم بضبط أذوناته على `sudo chmod 0600 /swapfile` وإستخدم `/swapfile` كـ `SWAPDEV` لبقية هذه التعليمات
  - قم بتهيئة الجهاز (Format) للإستخدام كمُبادلة مع `sudo mkswap SWAPDEV`
4. أضف ملف المُبادلة إلى `/etc/fstab` بسطر جديد يحتوي على `SWAPDEV swap swap defaults 0 0`
5. قُم بتمكين التبديل بـ `sudo swapon -a` وقم بتركيب tmpfs بـ `sudo mount /mnt/solana-accounts/`
6. تأكد من أن المُبادلة نشطة مع `free -g` وأن tmpfs مُثبت بـ `mount`

أضف الآن الوسيطة `--accounts /mnt/solana-accounts` إلى وسيطات سطر الأوامر `solana-validator` وأعد تشغيل المُدقّق (validator).

### فهرسة الحساب (Account indexing)

نظرًا لتزايد عدد الحسابات المأهولة في المجموعة، فإن طلبات RPC لبيانات الحساب التي تفحص مجموعة الحساب بالكامل مثل  -- الحصول على حسابات البرنامج [`getProgramAccounts`](developing/clients/jsonrpc-api.md#getprogramaccounts) و الطلبات المُحَدَّدَة للرمز [SPL-token-specific requests](developing/clients/jsonrpc-api.md#gettokenaccountsbydelegate) -- قد يكون أداؤها ضعيفًا. إذا إحتاج المُدقّق (validator) الخاص بك إلى دعم أي من هذه الطلبات، فيُمكنك إستخدام المُعلمة `--account-index` لتنشيط فهرس حساب واحد أو أكثر في الذاكرة يعمل على تحسين أداء RPC بشكل ملحوظ عن طريق فهرسة الحسابات (indexing accounts) حسب حقل المفتاح. يدعم حاليًا قيم المُعلِّمات (parameters) التالية:

- مُعرف البرنامج `program-id`: كل حساب مُفهرس بواسطة البرنامج الخاص به؛ يستخدمه [`getProgramAccounts`](developing/clients/jsonrpc-api.md#getprogramaccounts)
- سَكّ الرمز `spl-token-mint`: كل حساب رمز لـ SPL مُفهرس بواسطة رمز السَّكّ (Mint) الخاص به؛ يستخدمه للحصول على حسابات الرمز المميز عن طريق التفويض [getTokenAccountsByDelegate](developing/clients/jsonrpc-api.md#gettokenaccountsbydelegate) و الحصول على أكبر حسابات الرمز [getTokenLargestAccounts](developing/clients/jsonrpc-api.md#gettokenlargestaccounts)
- `spl-token-owner`: كل حساب رمز SPL مُفهرس بواسطة عنوان مالك الرمز؛ مُستخدم بواسطة طلبات الحصول على حسابات الرمز حسب المالك [getTokenAccountsByOwner](developing/clients/jsonrpc-api.md#gettokenaccountsbyowner) و الحصول على حسابات البرنامج [`getProgramAccounts`](developing/clients/jsonrpc-api.md#getprogramaccounts) التي تتضمن فلتر spl-token-owner.
