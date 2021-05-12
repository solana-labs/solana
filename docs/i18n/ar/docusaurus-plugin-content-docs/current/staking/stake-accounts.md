---
title: هيكل حساب إثبات الحِصَّة أو التَّحْصِيص (Stake Account Structure)
---

يُمكن إستخدام حساب حِصَّة (stake account) على Solana لتفويض الرموز للمُدقّقين (validators) على الشبكة لكسب مكافآت محتملة لمالك حساب الحِصَّة. Stake accounts are created and managed differently than a traditional wallet address, known as a _system account_. حساب النظام قادر فقط على إرسال وتلقي SOL من حسابات أخرى على الشبكة، بينما يدعم حساب الحِصَّة عمليات أكثر تعقيدًا مطلوبة لإدارة تفويض الرموز.

تعمل حسابات الستاك على شبكة سولانا أيضًا بشكل مختلف عن تلك الخاصة بشبكات البلوكشاين الأخرى التي قد تكون على دراية بها. تصف هذه الوثيقة بنية ووظائف عالية المستوى لحساب حِصَّة Solana.

#### عنوان الحساب (Account Address)

لكل حساب حِصَّة عنوان فريد يمكن إستخدامه للبحث عن معلومات الحساب في سطر الأوامر أو في أي أدوات مستكشف الشبكة. مع ذلك، على عكس عنوان المحفظة الذي يتحكم فيه صاحب العنوان الرئيسي في المحفظة، فإن زوج المفاتيح المرتبط بعنوان حساب الحِصَّة ليس له بالضرورة أي سيطرة على الحساب. في الواقع، قد لا يوجد زوج مفتاح (keypair) أو مفتاح خاص (private key) لعنوان حساب حِصَّة.

المرة الوحيدة التي يحتوي فيها عنوان حساب حِصَّة على ملف زوج مفاتيح هي عند إنشاء حساب حِصَّة بإستخدام أدوات سطر الأوامر [creating a stake account using the command line tools](../cli/delegate-stake.md#create-a-stake-account)، إذ يتم إنشاء ملف زوج مفاتيح (keypair) جديد أولاً فقط للتأكد من أن عنوان حساب الحِصَّة جديد وفريد.

#### فهم صلاحيات الحساب

Certain types of accounts may have one or more _signing authorities_ associated with a given account. يتم استخدام سلطة الحساب لتوقيع معاملات معينة للحساب الذي يتحكم فيه. يختلف هذا عن بعض شبكات البلوكشين الأخرى حيث يتحكم صاحب زوج المفاتيح المرتبط بعنوان الحساب في جميع أنشطة الحساب.

لكل حساب حصَّة سلطتا توقيع مُحَدَّدَتان من خلال عنوان كل منهما، وكل منهما مخولة بإجراء عمليات معينة على حساب الحصَّة.

The _stake authority_ is used to sign transactions for the following operations:

- تفويض الحِصَّة
- إبطال مفعول تفويض الحِصَّة
- تقسيم حساب الحِصَّة، وإنشاء حساب حِصَّة جديد مع جزء من الأموال في الحساب الأول
- Merging two stake accounts into one
- إعداد سُلطة إثبات حِصَّة أو تَحْصِيص جديدة

The _withdraw authority_ signs transactions for the following:

- سحب الحِصَّة غير المفوضة إلى عنوان المحفظة
- إعداد تفويض سحب جديد
- إعداد سُلطة إثبات حِصَّة أو تَحْصِيص جديدة

يتم تعيين سلطة المشاركة وسلطة السحب عند إنشاء حساب الحِصَّة، ويمكن تغييرها لتفويض عنوان توقيع جديد في أي وقت. يمكن أن تكون الحِصَّة وسلطة السحب نفس العنوان أو عنوانين مختلفين.

يحتفظ زوج مفاتيح سلطة السحب بمزيد من التحكم في الحساب حيث إنه ضروري لتصفية الرموز في حساب الحِصَّة، ويمكن إستخدامه لإعادة تعيين سلطة الحِصَّة في حالة فقدان مفتاح سلطة الحِصَّة أو تعرضه للخطر.

يعد تأمين سلطة السحب ضد الضياع أو السرقة أمرًا في غاية الأهمية عند إدارة حساب مشاركة.

#### التفويضات المُتَعَدِّدَة (Multiple Delegations)

يمكن إستخدام كل حساب حِصَّة فقط للتفويض إلى مُدقّق واحد في كل مرة. جميع الرموز في الحساب إما مُفَوَّضَة أو غير مُفَوَّضَة، أو في طور التفويض أو عدم التفويض. لتفويض جزء من الرموز الخاصة بك إلى مُدقّق، أو للتفويض إلى العديد من المُدقّقين، يجب عليك إنشاء حسابات حِصَّة مُتَعَدِّدَة.

يمكن تحقيق ذلك عن طريق إنشاء حسابات حصة متعددة من عنوان محفظة يحتوي على بعض الرموز ، أو عن طريق إنشاء حساب حِصَّة كبير واحد وإستخدام سلطة الحِصَّة لتقسيم الحساب إلى حسابات مُتَعَدِّدَة بأرصدة رموز من إختيارك.

يمكن تعيين نفس صلاحيات الحِصَّة والسحب إلى حسابات حِصَص مُتَعَدِّدَة.

#### Merging stake accounts

Two stake accounts that have the same authorities and lockup can be merged into a single resulting stake account. A merge is possible between two stakes in the following states with no additional conditions:

- two deactivated stakes
- an inactive stake into an activating stake during its activation epoch

For the following cases, the voter pubkey and vote credits observed must match:

- two activated stakes
- two activating accounts that share an activation epoch, during the activation epoch

All other combinations of stake states will fail to merge, including all "transient" states, where a stake is activating or deactivating with a non-zero effective stake.

#### Delegation Warmup and Cooldown

When a stake account is delegated, or a delegation is deactivated, the operation does not take effect immediately.

A delegation or deactivation takes several [epochs](../terminology.md#epoch) to complete, with a fraction of the delegation becoming active or inactive at each epoch boundary after the transaction containing the instructions has been submitted to the cluster.

There is also a limit on how much total stake can become delegated or deactivated in a single epoch, to prevent large sudden changes in stake across the network as a whole. Since warmup and cooldown are dependent on the behavior of other network participants, their exact duration is difficult to predict. Details on the warmup and cooldown timing can be found [here](../cluster/stake-delegation-and-rewards.md#stake-warmup-cooldown-withdrawal).

#### Lockups

Stake accounts can have a lockup which prevents the tokens they hold from being withdrawn before a particular date or epoch has been reached. While locked up, the stake account can still be delegated, un-delegated, or split, and its stake and withdraw authorities can be changed as normal. Only withdrawal into a wallet address is not allowed.

A lockup can only be added when a stake account is first created, but it can be modified later, by the _lockup authority_ or _custodian_, the address of which is also set when the account is created.

#### Destroying a Stake Account

Like other types of accounts on the Solana network, a stake account that has a balance of 0 SOL is no longer tracked. If a stake account is not delegated and all of the tokens it contains are withdrawn to a wallet address, the account at that address is effectively destroyed, and will need to be manually re-created for the address to be used again.

#### Viewing Stake Accounts

Stake account details can be viewed on the Solana Explorer by copying and pasting an account address into the search bar.

- http://explorer.solana.com/accounts
