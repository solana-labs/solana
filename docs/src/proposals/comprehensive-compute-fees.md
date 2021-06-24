---
title: Comprehensive Compute Fees
---

## Motivation

The current fee structure lacks a comprehensive account of the work required by
a validator to process a transaction.  The fee structure is only based on the
number of signatures in a transaction but is meant to account for the work that
the validator must perform to validate each transaction.  The validator performs
a lot more user-defined work than just signature verification.  The work to
process a transaction typically includes signature verifications, account
locking, account loading, and instruction processing.

## Proposed Solution

### New fee structure

In addition to signature checking, the other work required to process an
instruction should be taken into account.  To do this, the total fee should be
an accumulated cost of all the work.  Each piece of work can be
measured/accounted for in a way that makes sense for the type of work involved.

A new fee structure could include:
1. flat fee per signature
2. flat fee for each write lock
3. per-byte fee for the amount of data in each loaded account
4. pre-calculated fee for builtin-in program instructions
5. per-compute-unit fee based on the measured number of compute units used to
   process an instruction

Fees 1-3 can be determined upfront before the message is processed.  #4 could be
measured and applied, probably by the program itself.  #5 would need to be
finally accounted for after the message is processed, but the payer's balance
could be pre-deducted against a compute budget cap for #4 and #5 to ensure they
have enough lamports to cover a max fee, and then any unused compute budget
could be credited back after the message is fully processed.

The goal of the fees is to cover the computation cost of processing a
transaction.  Each of the above fee categories could be represented as a compute
unit cost that, when added together, encompasses the entire cost of processing
the transaction.  By calculating the total cost of the transaction the runtime
can make better decisions on what transactions to process and when, as well as
throw out transactions that exceed a cap.

The per-compute unit fee doesn't have to be linear; developers could be
non-linearly incentivized to reduce compute costs by optimizing in any of the 4
fee categories listed above.

To give developers some control over fees and the compute cap, a new built-in
instruction could be introduced that requests a specific transaction-wide
compute budget cap.  The instruction can be used by a developer to reduce the
cap and thus fees they expect to pay, or to request a higher than the default
cap.  The runtime could in-turn, use these instructions to determine how to
schedule these more expensive transactions.

### Transaction compute caps

The current compute caps are independently applied to individual instructions.
This means the overall transaction cap varies depending on how many instructions
are in the transaction.  Instead, a transaction-wide cap is probably more
appropriate.  One challenge of the transaction-wide cap is that each instruction
(program) could no longer expect to be given an equal amount of compute units
since the number of compute units will be based on the number of units already
consumed by earlier instructions in the message.  This will provide some
additional tuning and composability challenges for developers.

### Builtin program compute costs

Builtin programs do not incur any compute cost at the moment and are therefore
not accounted for even when a bpf program invokes a builtin program via cpi
(except the direct overhead cost of the cpi call itself).  Builtin programs
should incur a program compute cost like any other program.  The challenge is
how to quantify that compute cost.  One approach would be to measure the cost of
each builtin program's instructions.

## Proposed steps to implement

- Apply the compute budget cap across the entire transaction rather than
  per-instruction
- Convert the per-sig fee to a per-sig compute cost and incorporate it into the
  transaction-wide cap.  Initially, keep charging the current per-sig fee.
- Add compute costs for write locks and account data loading and incorporate
  those into the transaction-wide cap
- Remove the per-sig centric fee and start charging fees based on the
  transaction compute budget
- Rework how the bank decides which transactions to process and when based on
  the compute budget expectations
- Add a built-in instruction that requests the amount of compute budget up front
- Transaction simulation where the results include the compute costs of the
  transaction.  This could be granular so that each instruction's cost is
  reported.

## Things to ponder

- Can things like write locks be meaningfully converted into a compute cost/cap?
- Should account data size be accounted for in the compute cost/cap
- Calculating a transaction's fee upfront becomes more difficult, would probably
  have to be reworded to be max fee based on the compute cap.  Actual fee would
  be equal to or less than that.
- "Compute cost" is a lame name, ideas for something better?

## Mainnet beta transaction wide compute measurements

Over 4 1/2 hours of Mainnet beta monitoring with a 200k instruction cap:
675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8 (Radium v3?)
  - exceeded 79669 times
  - exceeded by between 1 and 800k units
5fNfvyp5czQVX77yoACa3JJVEhdRaWjPuazuWgjhTqEH (Mango!)
  - exceeded 60 times
  - exceeded by < 100k units

Both transactions contain 2 or more instructions and both are running the same program in each instruction

Takeaways:
- If the transactions were split into single instructions per transaction they would be below the tx-wide compute limit of 200k.

## Mainnet beta program performance measurements

```
Per-program instruction characteristics over 5 minutes of mainnet
Program                                                    count               Total us        total compute units     units/us
                                                                     max  med  avg   sd     max   med    avg     sd  med avg  sd

27haf8L6oxUeXrHrgEgsexjSY5hbVUWEmvv9Nyxg8vQv Radium v2       329    9082 1466 2298 3062  126131  58074  65212 24949   32  39  28
4MNPdKu9wFMvEeZBMt3Eipfs5ovVWTJb31pEXDJAAxX5                  45     264   82   86   36  789       789    789     0   10  8   22
675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8 (Radium v3?)   6183   15544  858 1310 1355  179980  43283  55484 31286   53  45  79
7vxeyaXGLqcp66fFShqUdHxdacp4k4kwUpRSSeoZLCZ4                  27    3411 2101 2232  531  149039 115065 114843 18152   53  50 173
9KEPoZmtHUrBbhWN1v1KWLMkkvwY6WLtAVUCPRtRjP4z                 483    3136  747  646  445   48069  35015  27749 14983   48  47 129
9xQeWvG816bUx9EPjHmaT23yvVM2ZWbrrpZb9PusVFin Serum v3     115445   31749  483  550  430   26388   2489   5232  5181    7   7   7
ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL                 114    1485  651  681  158   47894  27498  27820  3966   42  40 158
CBuCnLe26faBpcBP2fktp4rp8abpcAnTWft6ZrP5Q4T                  228    1188  121  240  285   35918   8010  11542  9716   63  54  94
DjVE6JNiYqPL2QXyCUUh8rNjHrbz9hXHNYt99MQ59qw1                  82    2058  813  845   25   34272  28994  29111  1768   36  34 113
EhhTKczWMGQt46ynNeRX1WfeagwwJd7ufHvCDjRxjo5Q                 634    1557  111  292  294   29485   6100  11707  8519   53  47 101
FjJ5mhdRWeuaaremiHjeQextaAw1sKWDqr3D7pXjgztv                  86    2484  951  968  226   40565  40565  40565     0   43  42 179
J21zqcffYQU2NUJrvAKhqpKZLQK1YaB9dY5kmcxkMQvQ                  72     952  332  334   96   10645  10315  10331   128   32  31 108
SwaPpA9LAaLfeLi3a68M4DjnLqgtticKg6CnyNwgAC8                  203    9947 1076 1198  896   66633  58706  57275  4676   53  47  61
TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA                72286    8453  101  112   98    4429   3531   3026   683   29  27  31
Vote111111111111111111111111111111111111111                        39278  121  160  400
11111111111111111111111111111111                                      24   11   11    3
```

- Handy compute unit conversions
  - 10,000 units/us => 20 us @ 200k max units => 50k TPS max
  - 250 units/us => 800 us @ 200k max units => 1250 TPS max (original tune
    target)
  - 25 units/us => 8000 us @ 200k max units => 125 TPS max
  - 15 units/us => 13333 us @ 200k max units => 75 TPS max
  - 7 units/us => 30000 us @ 200k max units => 30 TPS max

Observations:
- These numbers only include program execution (message processor, vm
  setup/takedown, parameter ser/de) but do not include account loading/storing
- Fairly high level of variability in program measurements (for example, vote
  program has a mx of 38ms with med of 121)
- Current compute budget is 200k units max units
  - At the measured median rate of 15 units/us the max program execution time
    should be 13ms
  - The original target was 250 units/us for a max program execution time of 800
    us
- Of the 196,243 instructions logged
  - ~600 above 100k compute units
  - ~1,700 above 50k
  - ~9,500 above 20k
  - ~2,400 above 10k
  - ~38,000 above 5k
  - ~196,200 above 2k
- Currently traffic is dominated by Serum v3 which is very optimized (median
  2489 units and 483 us)
- It's possible that some of the programs are not in the executor cache which
  will add a large time component loading the elf - Somewhere around 80 us of
  program overhead with 1 account containing zero data
  - Serializing large accounts can take a significant amount of time
- Raydium v3 transactions contain many high unit instructions taking multiple ms
  - In this sample period it appears that all programs besides Raydium would fit
  within a ~200k transaction-wide limit (currently set to ~200k instruction-wide
  limit)

Takeaways:
- Establish a targeted default max transaction unit cap
  - 200k proposed
  - Work with Raydium to break up their transactions and possibly help them
    optimize their programs
  - Can allow developer requested larger or small caps later as per this
    proposal
  - When things like account loading get incorporated into the compute budget
    raise the default cap accordingly.
- Establish a new unit/us rate that will be used to calculate and set the cost
  of new program operations (syscalls for example)
  - What should the target be for a program's max execution time?
