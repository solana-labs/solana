---
title: تثبيت برامج الكتلة والتحديثات (Cluster Software Installation and Updates)
---

يُطلب من المُستخدمين حاليًا إنشاء برنامج مجموعة solana بأنفسهم من مُستودع git وتحديثه يدويًا، وهو عرضة للخطأ وغير مريح.

يقترح هذا المُستند وسيلة سهلة لتثبيت البرامج ومُحدِّثها (updater) والتي يمكن إستخدامها لنشر ثنائيات (binaries) المُضمنة مُسبقا (pre-built) للأنظمة الأساسية المدعومة. يجوز للمستخدمين اختيار إستخدام الثنائيات التي توفرها Solana أو أي طرف آخر يثقون فيه. تتم إدارة نشر التحديثات بإستخدام برنامج بيان التحديث على على الشبكة (on-chain.

## أمثلة مُحفزة (Motivating Examples)

### قم بإحضار وتشغيل برنامج التثبيت المبني مُسبقًا بإستخدام النصيسكريبت أو البرنامج النصي bootstrap curl / shell

أسهل طريقة تثبيت للمنصات المدعومة:

```bash
$ curl -sSf https://raw.githubusercontent.com/solana-labs/solana/v1.0.0/install/solana-install-init.sh | sh
```

سيقوم هذا السكريبت أو البرنامج النصي بفحص github للحصول على أحدث إصدار ذو علامات وتنزيل وتشغيل `solana-install-init` الـ binary من هناك.

إذا كان من الضروري تحديد وسيطات إضافية أثناء التثبيت، فسيتم إستخدام بناء جملة shell التالية:

```bash
$ init_args=.... # arguments for `solana-install-init ...`
$ curl -sSf https://raw.githubusercontent.com/solana-labs/solana/v1.0.0/install/solana-install-init.sh | sh -s - ${init_args}
```

### قم بإحضار وتشغيل برنامج التثبيت المُضمن مُسبقا (pre-built) من إصدار Github

بإستخدام عنوان الـ URL للإصدار المعروف، يمكن الحصول على binary مُضمن مُسبقا (pre-built) للأنظمة الأساسية المدعومة:

```bash
$ curl -o solana-install-init https://github.com/solana-labs/solana/releases/download/v1.0.0/solana-install-init-x86_64-apple-darwin
$ chmod +x ./solana-install-init
$ ./solana-install-init --help
```

### بناء وتشغيل المُثبت (installer) من المصدر

إذا لم يكن برنامج الـ binary المُضمن مُسبقا (pre-built) مُتاحًا لمنصة ممُعينة، فإن إنشاء المُثبت (installer) من المصدر يكون دائمًا خيارًا جيدا متاحا:

```bash
$ git clone https://github.com/solana-labs/solana.git
$ cd solana/install
$ cargo run -- --help
```

### نشر تحديث جديد إلى نظام مجموعة (Deploy a new update to a cluster)

بالنظر إلى إصدار tarball الخاص بـ solana \ (كما تم إنشاؤه بواسطة `ci/publish-tarball.sh`\) والذي تم تحميله بالفعل إلى عنوان URL مُتاح للجميع، ستنشر الأوامر التالية التحديث:

```bash
$ solana-keygen new -o update-manifest.json  # <-- only generated once, the public key is shared with users
$ solana-install deploy http://example.com/path/to/solana-release.tar.bz2 update-manifest.json
```

### قم بتشغيل عُقدة مُدقّق (validator node) تقوم بتحديث نفسها تلقائيًا

```bash
$ solana-install init --pubkey 92DMonmBYXwEMHJ99c9ceRSpAmk9v6i3RdvDdXaVcrfj  # <-- pubkey is obtained from whoever is deploying the updates
$ export PATH=~/.local/share/solana-install/bin:$PATH
$ solana-keygen ...  # <-- runs the latest solana-keygen
$ solana-install run solana-validator ...  # <-- runs a validator, restarting it as necesary when an update is applied
```

## بيان التحديث على على الشبكة (On-chain Update Manifest)

يتم إستخدام بيان التحديث للإعلان عن نشر tarballs الإصدار الجديد على مجموعة solana. يتم تخزين بيان التحديث بإستخدام برنامج `config`، ويصف كل حساب بيان تحديث قناة تحديث منطقية لثلاثية الهدف المُحددة \ (على سبيل المثال، `x86_64-apple-darwin`\). المفتاح العمومي (public key) للحساب معروف جيدًا بين الكيان الذي ينشر تحديثات جديدة والمُستخدمين الذين يستهلكون هذه التحديثات.

تتم إستضافة تحديث الـ tarball نفسها في مكان آخر، خارج الشبكة (off-chain) ويمكن جلبها من الرقم `download_url`. المُحدد.

```text
use solana_sdk::signature::Signature;

/// Information required to download and apply a given update
pub struct UpdateManifest {
    pub timestamp_secs: u64, // When the release was deployed in seconds since UNIX EPOCH
    pub download_url: String, // Download URL to the release tar.bz2
    pub download_sha256: String, // SHA256 digest of the release tar.bz2 file
}

/// Data of an Update Manifest program Account.
#[derive(Serialize, Deserialize, Default, Debug, PartialEq)]
pub struct SignedUpdateManifest {
    pub manifest: UpdateManifest,
    pub manifest_signature: Signature,
}
```

لاحظ أن الحقل `manifest` نفسه يحتوي على توقيع مُطابق \ (`manifest_signature`\) للحماية من هجمات الرجل في الوسط (man-in-the-middle) بين أداة `solana-install` وواجهة برمجة تطبيقات (API) الـ RPC لمجموعة solana.

للحماية من هجمات التراجع (rollback attacks)، سيرفض `solana-install` تثبيت تحديث أقدم `timestamp_secs` مما هو مثبت حاليًا.

## إصدار مُحتويات الأرشيف (Release Archive Contents)

من المُتوقع أن يكون أرشيف الإصدارات عبارة عن ملف tar مضغوط بواسطة bzip2 بالهيكل الداخلي التالي:

- `/version.yml` - ملف YAML بسيط يحتوي على الحقل الهدف `"target"` -

  المجموعة الهدف. يتم تجاهل أي حقول إضافية.

- `/bin/` -- دليل يحتوي على البرامج المُتاحة في الإصدار.

  سيربط `solana-install` هذا الدليل إلى

  `~/.local/share/solana-install/bin` للإستخدام بواسطة بيئة `PATH`

  المُتغير.

- `... ` - يُسمح بأي ملفات وأدلة إضافية

## أداة تثبيت solana

يتم إستخدام أداة التثبيت `solana-install` بواسطة المُستخدم لتثبيت برنامج المجموعة (cluster) الخاص به وتحديثه.

يُدير الملفات والأدلة التالية في الدليل الرئيسي للمُستخدم:

- `~/.config/solana/install/config.yml` - تكوين المُستخدم ومعلومات حول إصدار البرنامج المُثبت حاليًا
- `~/.local/share/solana/install/bin` - رابط للإصدار الحالي. على سبيل المثال، `~/.local/share/solana-update/<update-pubkey>-<manifest_signature>/bin`
- `~/.local/share/solana/install/releases/<download_sha256>/` - مُحتويات الإصدار

### واجهة سطر الأوامر (Command-line Interface)

```text
solana-install 0.16.0
The solana cluster software installer

USAGE:
    solana-install [OPTIONS] <SUBCOMMAND>

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -c, --config <PATH>    Configuration file to use [default: .../Library/Preferences/solana/install.yml]

SUBCOMMANDS:
    deploy    deploys a new update
    help      Prints this message or the help of the given subcommand(s)
    info      displays information about the current installation
    init      initializes a new installation
    run       Runs a program while periodically checking and applying software updates
    update    checks for an update, and if available downloads and applies it
```

```text
solana-install-init
initializes a new installation

USAGE:
    solana-install init [OPTIONS]

FLAGS:
    -h, --help    Prints help information

OPTIONS:
    -d, --data_dir <PATH>    Directory to store install data [default: .../Library/Application Support/solana]
    -u, --url <URL>          JSON RPC URL for the solana cluster [default: http://devnet.solana.com]
    -p, --pubkey <PUBKEY>    Public key of the update manifest [default: 9XX329sPuskWhH4DQh6k16c87dHKhXLBZTL3Gxmve8Gp]
```

```text
info solana-install
displays information about the current installation

USAGE:
    solana-install info [FLAGS]

FLAGS:
    -h, --help     Prints help information
    -l, --local    only display local information, don't check the cluster for new updates
```

```text
solana-install deploy
deploys a new update
USAGE:
    solana-install deploy <download_url> <update_manifest_keypair>

FLAGS:
    -h, --help    Prints help information

ARGS:
    <download_url>               URL to the solana release archive
    <update_manifest_keypair>    Keypair file for the update manifest (/path/to/keypair.json)
```

```text
update solana-install
checks for an update, and if available downloads and applies it

USAGE:
    solana-install update

FLAGS:
    -h, --help    Prints help information
```

```text
solana-install run
Runs a program while periodically checking and applying software updates

USAGE:
    solana-install run <program_name> [program_arguments]...

FLAGS:
    -h, --help    Prints help information

ARGS:
    <program_name>            program to run
    <program_arguments>...    arguments to supply to the program

The program will be restarted upon a successful software update
```
