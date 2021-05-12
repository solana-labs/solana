---
title: تنصيب مجموعة Solana Tool Suite
---

هناك طرق مُتعددة لتنصيب أدوات Solana على جهاز الكمبيوتر الخاص بك إعتماداً على سير العمل المُفضل:

- [إستخدام أداة Solana's Install Tool (أبسط خيار)](#use-solanas-install-tool)
- [تنزيل الثنائيات المُدمجة مُسبَّقا (Prebuilt binaries)](#download-prebuilt-binaries)
- [بناء من المصدر](#build-from-source)

## إستخدام أداة Solana's Install Tool

### نظامي التشغيل MacOS و Linux

- إفتح تطبيق الTerminal المُفضل لديك

- تثبيت إصدار Solana [LATEST_SOLALANA_RELEASE_VERSION](https://github.com/solana-labs/solana/releases/tag/LATEST_SOLANA_RELEASE_VERSION) على جهازك بتشغيل:

```bash
sh -c "$(curl -sSfL https://release.solana.com/LATEST_SOLANA_RELEASE_VERSION/install)"
```

- يُمكنك إستبدال `LATEST_SOLALANA_RELEASE_VERSION` بالإصدار الذي يُطابق نُسخة البرنامج من الإصدار المطلوب، أو إستخدم أحد أسماء القنوات الرمزية الثلاثة: `stable`, `beta`, or `edge`.

- تُشير المُخرجات التالية إلى نجاح التحديث:

```text
تنزيل أداة التثبيت LATEST_SOLANA_RELEASE_VERSION
الإعدادات: /home/solana/.config/solana/install/config.yml
مسار الإصدار الحالي: /home/solana/.local/share/solana/install/active_release
* نسخة الإصدار: LATEST_SOLANA_RELEASE_VERSION
* رابط الإصدار: https://github.com/solana-labs/solana/releases/download/LATEST_SOLANA_RELEASE_VERSION/solana-release-x86_64-unknown-linux-gnu.tar.bz2
تم التحديث بنجاح
```

- إعتمادا على النظام الخاص بك، قد تظهر لك رسالة في آخر عملية التثبيت وتدعوك إلى

```bash
يُرجى تحديث مُتغير بيئة الPATH الخاص بك لتضمينه برامج solana:
```

- إذا حصلت على الرسالة الواردة أعلاه، قُم بنسخ ولصق الأمر المُوصى به أدناه لتحديث الـ `PATH`
- تأكد من أن لديك الإصدار المطلوب من `solana` المُنصب عن طريق تشغيل:

```bash
solana --version
```

- بعد عملية تنصيب ناجحة، `solana-install update` يُمكن إستخدامه بسهولة لتحديث برنامج Solana إلى إصدار أحدث في أي وقت.

---

### نظام التشغيل Windows

- قُم بفتح مُوججِّه الأوامر Command Prompt (`cmd.exe`) كAdministrator

  - البحث عن مُوجِّه الأوامر (Command Prompt) في شريط البحث Windows. عندما يظهر مُوجِّه الأوامر (Command Prompt)، قُم بالنقر بزر الفأرة (Mouse) الأيمن وحدد "Open as Administrator". إذا تمت مُطالبتك من خلال نافذة مُنبثقة (pop-up window) تسألك "هل تريد السماح لهذا التطبيق إجراء تغييرات على جهازك؟ "، قُم باانقر فوق" نعم " (Yes).

- قُم بنسخ ولصق الأمر التالي، ثم إضغط على Enter لتحميل Solana installer في الtemporary directory:

```bash
curl https://release.solana.com/LATEST_SOLANA_RELEASE_VERSION/solana-install-init-x86_64-pc-windows-msvc.exe --output C:\solana-install-tmp\solana-install-init.exe --create-dirs
```

- قُم بنسخ ولصق الأمر التالي، ثم إضغط Enter لتثبيت أحدث إصدار من Solana. إذا رأيت نافذة تحذير أمني مُنبثقة بنظامك، فيُرجى إختيار وتحديد السماح بتشغيل البرنامج.

```bash
C:\solana-install-tmp\solana-install-init.exe LATEST_SOLANA_RELEASE_VERSION
```

- عند إنتهاء مُثَبِّت أو مُسَطِّب (installer) البرنامج، إضغط على Enter.

- إغلاق نافذة مُوجِّه الأوامر (Command Prompt) وإعادة فتح نافذة مُوجِّه الأوامر الجديدة كمُستخدم عادي (normal user)
  - إبحث عن "مُوجِّه الأوامر " (Command Prompt) في شريط البحث، ثم قُم بالنقر بزر الفأرة (Mouse) الأيسر على رمز تطبيق مُوجِّه الأوامر (Command Prompt)، لا حاجة للتشغيل كAdministrator)
- تأكد من أن لديك الإصدار المُنَصَّب المطلوب من `solana` عن طريق تشغيل:

```bash
solana --version
```

- بعد عملية تنصيب ناجحة، `solana-install update` يُمكن إستخدامه بسهولة لتحديث برنامج Solana إلى إصدار أحدث في أي وقت.

## تنزيل الثنائيات المُدمجة مُسبَّقا (Prebuilt binaries)

إذا كنت تُفضل عدم إستخدام `solana-install` لإدارة التثبيت، فيُمكنك تنزيل وتثبيت الثنائيات (binaries) يدويا.

### نظام التشغيل Linux

قُم بتنزيل الثنائيات (binaries)بالانتقال إلى [https://github.com/solana-labs/solana/releases/latest](https://github.com/solana-labs/solana/releases/latest)، تنزيل **solana-release-x86_64-unknown-linux-msvc.tar.bz2**، ثم إستخرج الأرشيف:

```bash
tar jxf solana-release-x86_64-unknown-linux-gnu.tar.bz2
cd solana-release/
export PATH=$PWD/bin:$PATH
```

### نظام التشغيل MacOS

قُم بتنزيل الثنائيات (binaries) بالانتقال إلى [https://github.com/solana-labs/solana/releases/latest](https://github.com/solana-labs/solana/releases/latest)، وتنزيل **solana-release-x86_64-apple-darwin.tar.bz2**، ثم إستخرج الأرشيف:

```bash
tar jxf solana-release-x86_64-apple-darwin.tar.bz2
cd solana-release/
export PATH=$PWD/bin:$PATH
```

### نظام التشغيل Windows

- قُم بتنزيل الثنائيات (binaries) بالانتقال إلى [https://github.com/solana-labs/solana/releases/latest](https://github.com/solana-labs/solana/releases/latest)، بتنزيل **solana-release-x86_64-pc-windows-msvc.tar.bz2**، ثم إستخرج الأرشيف بإستخدام برنامج WinZip أو ما شابه.

- قُم بفتح مُوجِّه الأوامر (Command Prompt) وإنتقل إلى المسار الذي إستخرجت منه الثنائيات (binaries) وقُم بتشغيل:

```bash
cd solana-release/
set PATH=%cd%/bin;%PATH%
```

## بناء من المصدر

إذا كنت غير قادر على إستخدام الprebuilt binaries أو تفضل إنشاءها بنفسك من المصدر، إنتقل إلى [ https://github.com/solana-labs/solana/releases/latest ](https://github.com/solana-labs/solana/releases/latest)، وتنزيل أرشيف **Source Code**. إستخراج التعليمات البرمجية وبناء الbinaries بواسطة:

```bash
./scripts/cargo-install-all.sh .
export PATH=$PWD/bin:$PATH
```

يمكنك بعد ذلك تشغيل الأمر التالي للحصول على نفس النتيجة مع الprebuilt binaries:

```bash
solana-install init
```
