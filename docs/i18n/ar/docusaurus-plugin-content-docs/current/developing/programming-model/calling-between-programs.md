---
title: الإتصال بين البرامج (Calling Between Programs)
---

## الإستدعاءات عبر البرامج (Cross-Program Invocations)

يسمح وقت تشغيل Solana للبرامج بالإتصال ببعضها البعض عن طريق آلية تسمى الإستدعاءات عبر البرامج. يتم إجراء الإتصال بين البرامج من خلال أحد البرامج الذي يستدعي تعليمات من البرنامج الآخر. يتم إيقاف برنامج الإستدعاء حتى ينتهي البرنامج الذي تم إستدعاؤه من معالجة التعليمات.

على سبيل المثال، يمكن للعميل إنشاء معاملة تُعدل حسابين، يمتلك كل منهما برامج مُنفصلة على على الشبكة (on-chain):

```rust,ignore
let message = Message::new(vec![
    token_instruction::pay(&alice_pubkey),
    acme_instruction::launch_missiles(&bob_pubkey),
]);
client.send_and_confirm_message(&[&alice_keypair, &bob_keypair], &message);
```

يجوز للعميل بدلاً من ذلك السماح لبرنامج `acme` بإستدعاء تعليمات `token` نيابة عن العميل:

```rust,ignore
let message = Message::new(vec![
    acme_instruction::pay_and_launch_missiles(&alice_pubkey, &bob_pubkey),
]);
client.send_and_confirm_message(&[&alice_keypair, &bob_keypair], &message);
```

بالنظر إلى برنامجين على الشبكة `token` و `acme`، كل منهما يقوم بتنفيذ التعليمات `pay()` و `launch_missiles()` على التوالي ،يمكن تنفيذ acme باستدعاء وظيفة محددة في الوحدة `token` عن طريق إصدار استدعاء عبر البرامج:

```rust,ignore
mod acme {
    use token_instruction;

    fn launch_missiles(accounts: &[AccountInfo]) -> Result<()> {
        ...
    }

    fn pay_and_launch_missiles(accounts: &[AccountInfo]) -> Result<()> {
        let alice_pubkey = accounts[1].key;
        let instruction = token_instruction::pay(&alice_pubkey);
        invoke(&instruction, accounts)?;

        launch_missiles(accounts)?;
    }
```

تم تضمين `invoke()` في وقت تشغيل Solana وهو مسؤول عن توجيه التعليمات المقدمة إلى برنامج `token` عبر حقل التعليمات `program_id`.

لاحظ أن `invoke` يتطلب من المُتصل تمرير جميع الحسابات المطلوبة من خلال التعليمات التي يتم إستدعاءها. هذا يعني أنه تم تمرير كل من الحساب القابل للتنفيذ (الحسابات التي تتطابق مع مُعرف برنامج التعليمات) والحسابات إلى مُعالج التعليمات.

قبل إستدعاء `pay() ` ، يجب أن يضمن وقت التشغيل أن `acme` لم يُعدل أي حسابات مملوكة لـ `token`. يقوم بذلك عن طريق تطبيق سياسة وقت التشغيل على الحالة الحالية للحسابات في الوقت الذي يستدعي `acme` فيه `invoke` مُقابل الحالة الأولية للحسابات في بداية تعليمات `acme`. بعد إكتمال `pay()`، يجب أن يضمن وقت التشغيل مرة أخرى أن `token` لم يُعدل أي حسابات مملوكة لـ `acme` من خلال تطبيق سياسة وقت التشغيل مرة أخرى، ولكن هذه المرة بـ `token` مُعرف البرنامج. أخيرًا، بعد إكتمال `pay_and_launch_missiles()`، يجب أن يُطبق وقت التشغيل سياسة وقت التشغيل مرة أخرى، حيث يكون عادةً، ولكن بإستخدام جميع المتغيرات `pre_*` المحدثة. إذا لم يجر تنفيذ `pay_and_launch_missiles()` حتى `pay()` أي تغييرات غير صالحة في الحساب، ولم يجرِ `pay()` أي تغييرات غير صالحة، والتنفيذ من `pay()` حتى `pay_and_launch_missiles()` لم يُنتج عنه أي تغييرات غير صالحة، عندئذٍ يمكن لوقت التشغيل أن يفترض بشكل إنتقالي أن `pay_and_launch_missiles()` ككل لم يجر أي تغييرات غير صالحة في الحساب، وبالتالي الإلتزام بكل تعديلات هذه الحساب.

### التعليمات التي تتطلب إمتيازات

يستخدم وقت التشغيل الإمتيازات الممنوحة لبرنامج المُتَّصِل (caller) لتحديد الإمتيازات التي يمكن توسيعها للمُتَّصَل به (callee). الإمتيازات في هذا السياق يُشير إلى مُوقِّعين (signers) وحسابات قابلة للكتابة. على سبيل المثال، إذا كانت التعليمات التي يقوم المُتَّصِل (caller) بمعالجتها تحتوي على موقع أو حساب قابل للكتابة، فيمكن للمُتَّصِل (caller) إستدعاء تعليمات تحتوي أيضًا على ذلك المُوقِّع (signer) و / أو الحساب القابل للكتابة.

يعتمد هذا الإمتياز على حقيقة أن البرامج ثابتة. في حالة برنامج `acme`، يمكن لوقت التشغيل التعامل بأمان مع توقيع المُعاملة بإعتباره توقيعًا لتعليمات `token`. عندما يرى وقت التشغيل `token` تُشير التعليمات إلى `alice_pubkey`، فإنه يبحث عن المفتاح في تعليمات `acme` لمعرفة ما إذا كان هذا المفتاح يتوافق مع حساب مُوقَّع. في هذه الحالة، يقوم بذلك وبالتالي يصرح لبرنامج `token` بتعديل حساب Alice.

### برنامج الحسابات المُوقَّعة (Program signed accounts)

يُمكن للبرامج إصدار إرشادات تحتوي على حسابات مُوقَّعة لم يتم تسجيلها في المُعاملة الأصلية بإستخدام [Program derived addresses](#program-derived-addresses).

للتوقيع على حساب بعناوين مُستمدة من البرنامج، يجوز للبرنامج `invoke_signed()`.

```rust,ignore
        invoke_signed(
            &instruction,
            accounts,
            &[&["First addresses seed"],
              &["Second addresses first seed", "Second addresses second seed"]],
        )؟;
```

### عمق الإتصال (Call Depth)

تسمح الإستدعاءات عبر البرامج (Cross-program invocations) للبرامج بإستدعاء برامج أخرى مُباشرة ولكن العمق مقيد حاليًا بـ 4.

### إعادة الحيازة (Reentrancy)

تقتصر إعادة الحيازة (Reentrancy) حاليًا على التكرار الذاتي المُباشر المحدد بعمق ثابت. يمنع هذا التقييد المواقف التي قد يستدعي فيها برنامج آخر من حالة وسيطة دون معرفة أنه قد يتم إستدعاؤه مرة أخرى لاحقًا. يمنح التكرار المباشر البرنامج التحكم الكامل في حالته عند النقطة التي يتم إستدعاؤه مرة أخرى.

## عناوين البرنامج المُشتقِّة (Program Derived Addresses)

تسمح العناوين المُشتقِّة من البرنامج بإستخدام التوقيع المُنشأ برمجيًا عند الإتصال بين البرامج [calling between programs](#cross-program-invocations).

بإستخدام عنوان مُشتقِّ من البرنامج، قد يُمنح البرنامج السلطة على حساب ما ثم ينقل هذه الصلاحية لاحقًا إلى حساب آخر. هذا ممكن لأن البرنامج يمكن أن يعمل كمُوَقِّع (signer) في المُعاملة التي تمنح السلطة.

على سبيل المثال، إذا أراد مُستخدمان المُراهنة على نتيجة لعبة في Solana، فيجب على كل منهما نقل أصول الرهان إلى وسيط يحترم إتفاقهما. لا توجد حاليًا طريقة لتنفيذ هذا الوسيط كبرنامج في Solana لأن البرنامج الوسيط لا يمكنه نقل الأصول إلى الفائز.

هذه الإمكانية ضرورية للعديد من تطبيقات الـ DeFi لأنها تتطلب نقل الأصول إلى وكيل ضمان حتى وقوع حدث ما يُحدد المالك الجديد.

- منصات التداول اللامركزية التي تنقل الأصول بين طلبات البيع والشراء المطابقة.

- المزادات التي تنقل الأصول إلى الفائز.

- الألعاب أو أسواق التنبؤ التي تجمع الجوائز وتُعيد توزيعها على الفائزين.

عنوان البرنامج المُشتقِّ (Program derived address):

1. السماح للبرامج بالتحكم في عناوين مُحددة، تُسمى عناوين البرامج، بحيث لا يمكن لأي مُستخدم خارجي إنشاء مُعاملات صالحة بتوقيعات تلك العناوين.

2. السماح للبرامج بالتوقيع برمجيًا لعناوين البرامج الموجودة في الإرشادات التي يتم إستدعاؤها عبر الدعوات عبر البرامج [Cross-Program Invocations](#cross-program-invocations).

بالنظر إلى الشرطين، يمكن للمُستخدمين نقل أو تعيين سلطة الأصول على الشبكة (on-chain) بشكل آمن لعناوين البرامج ويمكن للبرنامج بعد ذلك تعيين هذه السلطة في مكان آخر وفقًا لتقديره.

### مفاتيح خاصة (Private keys) لعناوين البرامج (program addresses)

لا يقع عنوان البرنامج على منحنى ed25519 وبالتالي لا يحتوي على مفتاح خاص (private key) صالح مُرتبط به، وبالتالي فإن إنشاء توقيع له أمر مستحيل. في حين أنه لا يحتوي على مفتاح خاص (private key) خاص به، إلا أنه يُمكن إستخدامه بواسطة برنامج لإصدار تعليمات تتضمن عنوان البرنامج كمُوَقِّع (signer).

### عناوين البرامج التي تم إنشاؤها على أساس التجزئة (Hash-based generated program addresses)

يتم إشتقاق عناوين البرنامج بشكل حاسم من مجموعة البذور (seeds) ومُعرف البرنامج (program id) بإستخدام وظيفة تجزئة 256-bit مُقاومة للصور المُسبقًة (256-bit pre-image resistant hash function). يجب ألا يقع عنوان البرنامج على منحنى ed25519 لضمان عدم وجود مفتاح خاص (private key) مُرتبط به. أثناء التوليد، سيتم إرجاع خطأ إذا تم العثور على العنوان على المُنحنى. يحدث تغيير بنسبة 50/50 لمجموعة مُعينة من البذور (seeds) ومُعرف البرنامج (program id). في حالة حدوث ذلك، يُمكن إستخدام مجموعة مُختلفة من البذور (seeds) أو نتوء البذور (بذرة 8 bit إضافية) للعثور على عنوان برنامج صالح خارج المُنحنى.

تتبع عناوين البرامج الحتمية للبرامج مسار إشتقاق مُشابهًا للحسابات التي تم إنشاؤها بإستخدام `SystemInstruction::CreateAccountWithSeed` والتي يتم تنفيذها بـ `system_instruction::create_address_with_seed`.

للإشارة فإن التنفيذ يتم على النحو التالي:

```rust,ignore
pub fn create_address_with_seed(
    base: &Pubkey,
    seed: &str,
    program_id: &Pubkey,
) -> Result<Pubkey, SystemError> {
    if seed.len() > MAX_ADDRESS_SEED_LEN {
        return Err(SystemError::MaxSeedLengthExceeded);
    }

    Ok(Pubkey::new(
        hashv(&[base.as_ref(), seed.as_ref(), program_id.as_ref()]).as_ref(),
    ))
}
```

يُمكن للبرامج أن تشتق بشكل حاسم أي عدد من العناوين بإستخدام البذور (seeds). يمكن لهذه البذور (seeds) أن تحدد رمزياً كيفية إستخدام العناوين.

من المفتاح العمومي `Pubkey`::

```rust,ignore
/// Generate a derived program address
///     * seeds, symbolic keywords used to derive the key
///     * program_id, program that the address is derived for
pub fn create_program_address(
    seeds: &[&[u8]],
    program_id: &Pubkey,
) -> Result<Pubkey, PubkeyError>
```

### إستخدام عناوين البرنامج (Using program addresses)

يمكن للعملاء إستخدام الدالة إنشاء عنوان البرنامج `create_program_address` لإنشاء عنوان وجهة.

```rust,ignore
// deterministically derive the escrow key
let escrow_pubkey = create_program_address(&[&["escrow"]], &escrow_program_id);

// construct a transfer message using that key
let message = Message::new(vec![
    token_instruction::transfer(&alice_pubkey, &escrow_pubkey, 1),
]);

// process the message which transfer one 1 token to the escrow
client.send_and_confirm_message(&[&alice_keypair], &message);
```

يمكن للبرامج أن تستخدم نفس الوظيفة لتوليد نفس العنوان. في الدالة أدناه البرنامج يطرح `token_instruction::transfer` من عنوان البرنامج كما لو كان لديه المفتاح الخاص (private key) لتوقيع المُعاملة.

```rust,ignore
fn transfer_one_token_from_escrow(
    program_id: &Pubkey,
    keyed_accounts: &[KeyedAccount]
) -> Result<()> {

    // User supplies the destination
    let alice_pubkey = keyed_accounts[1].unsigned_key();

    // Deterministically derive the escrow pubkey.
    let escrow_pubkey = create_program_address(&[&["escrow"]], program_id);

    // Create the transfer instruction
    let instruction = token_instruction::transfer(&escrow_pubkey, &alice_pubkey, 1);

    // The runtime deterministically derives the key from the currently
    // executing program ID and the supplied keywords.
    // If the derived address matches a key marked as signed in the instruction
    // then that key is accepted as signed.
    invoke_signed(&instruction,  &[&["escrow"]])؟
}
```

### التعليمات التي تتطلب مُوقِّعين (Instructions that require signers)

لا يمكن تمييز العناوين التي تم إنشاؤها بإستخدام إنشاء عنوان البرنامج `create_program_address` عن أي مفتاح عمومي (public key) آخر. الطريقة الوحيدة لوقت التشغيل للتحقق من أن العنوان ينتمي إلى البرنامج هي أن يقوم البرنامج بتزويد البذور (seeds) المُستخدمة لإنشاء العنوان.

سيستدعي وقت التشغيل دالة إنشاء عنوان البرنامج `create_program_address` داخليًا، ويُقارن النتيجة بالعناوين المتوفرة في التعليمات.

## أمثلة

يُرجى الرجوع إلى [Developing with Rust](developing/deployed-programs/../../../deployed-programs/developing-rust.md#examples) و [Developing with C](developing/deployed-programs/../../../deployed-programs/developing-c.md#examples) للحصول على أمثلة لكيفية إستخدام الإستدعاءات عبر البرامج (Cross-Program Invocations).
