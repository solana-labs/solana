---
title: График инфляции
---

**С учетом изменений. Следите за последними экономическими обсуждениями на форумах Solana: https://forums.solana.com**

Валидатор-клиенты имеют две функциональные роли в сети Solana:

- Валидация\ (голосование \) текущее глобальное состояние наблюдаемого PoH.
- Быть избранным «лидером» по круговому графику с взвешиванием по ставкам, в течение которого они несут ответственность за сбор невыполненных транзакций и включение их в наблюдаемую PoH, тем самым обновляя глобальное состояние сети и обеспечивая непрерывность цепочки.

Награды валидатора-клиента за эти услуги должны распределяться в конце каждой эпохи Solana. Как обсуждалось ранее, компенсация для клиентов-валидаторов предоставляется через комиссию, взимаемую с годового уровня инфляции на основе протокола, распределенного пропорционально весу ставок каждого узла-валидатора \ (см. ниже \) вместе с доступными комиссиями за транзакции, заявленными лидером. во время каждой ротации лидера. То есть пока данный клиент-валидатор избирается лидером, у него есть возможность удерживать часть комиссии за каждую транзакцию, за вычетом указанной в протоколе суммы, которая уничтожается \ (см. [Validation-client State Transaction Fees](ed_vce_state_validation_transaction_fees.md)\). \).

Эффективная годовая доходность от ставок на основе протокола \ (% \) за эпоху, полученная клиентами валидации, должна быть функцией:

- текущий глобальный уровень инфляции, полученный из предварительно определенного графика выпуска без инфляции \ (см. [Validation-client Economics](ed_vce_overview.md)\)
- доля стейка SOL в текущем общем обороте,
- комиссия, взимаемая сервисом проверки
- время работы / участие \ [% доступных слотов, за которые валидатор имел возможность проголосовать \] за данного валидатора за предыдущую эпоху.

Первый фактор является функцией только параметров протокола \ (т. Е. Не зависит от поведения валидатора в данную эпоху \) и приводит к графику инфляции, предназначенному для стимулирования раннего участия, обеспечения четкой денежно-кредитной стабильности и обеспечения оптимальной безопасности в сети.

As a first step to understanding the impact of the _Inflation Schedule_ on the Solana economy, we’ve simulated the upper and lower ranges of what token issuance over time might look like given the current ranges of Inflation Schedule parameters under study.

В частности:

- _Initial Inflation Rate_: 7-9%
- _Dis-inflation Rate_: -14-16%
- _Long-term Inflation Rate_: 1-2%

Используя эти диапазоны для моделирования ряда возможных графиков инфляции, мы можем исследовать инфляцию во времени:

![](/img/p_inflation_schedule_ranges_w_comments.png)

На приведенном выше графике указаны средние значения диапазона, чтобы проиллюстрировать вклад каждого параметра. From these simulated _Inflation Schedules_, we can also project ranges for token issuance over time.

![](/img/p_total_supply_ranges.png)

Finally we can estimate the _Staked Yield_ on staked SOL, if we introduce an additional parameter, previously discussed, _% of Staked SOL_:

%~\text{SOL Staked} = \frac{\text{Total SOL Staked}}{\text{Total Current Supply}}

In this case, because _% of Staked SOL_ is a parameter that must be estimated (unlike the _Inflation Schedule_ parameters), it is easier to use specific _Inflation Schedule_ parameters and explore a range of _% of Staked SOL_. Для примера ниже мы выбрали середину исследуемых выше диапазонов параметров:

- _Initial Inflation Rate_: 8%
- _Dis-inflation Rate_: -15%
- _Long-term Inflation Rate_: 1.5%

The values of _% of Staked SOL_ range from 60% - 90%, which we feel covers the likely range we expect to observe, based on feedback from the investor and validator communities as well as what is observed on comparable Proof-of-Stake protocols.

![](/img/p_ex_staked_yields.png)

Again, the above shows an example _Staked Yield_ that a staker might expect over time on the Solana network with the _Inflation Schedule_ as specified. This is an idealized _Staked Yield_ as it neglects validator uptime impact on rewards, validator commissions, potential yield throttling and potential slashing incidents. It additionally ignores that _% of Staked SOL_ is dynamic by design - the economic incentives set up by this _Inflation Schedule_.

### Скорректированная доходность стейкинга

A complete appraisal of earning potential from staking tokens should take into account staked _Token Dilution_ and its impact on staking yield. For this, we define _adjusted staking yield_ as the change in fractional token supply ownership of staked tokens due to the distribution of inflation issuance. То есть позитивное стимулирующее воздействие инфляции.

We can examine the _adjusted staking yield_ as a function of the inflation rate and the percent of staked tokens on the network. Здесь мы можем увидеть график для различных долей ставок:

![](/img/p_ex_staked_dilution.png)
