//! Logic for determining when rent should have been collected or a rewrite would have occurred for an account.
use {
    crate::{
        accounts_db::AccountsDb,
        accounts_hash::HashStats,
        bank::{Bank, PartitionIndex, Rewrites},
        rent_collector::{RentCollector, RentResult},
    },
    solana_sdk::{
        account::{AccountSharedData, ReadableAccount, WritableAccount},
        clock::{Epoch, Slot},
        epoch_schedule::EpochSchedule,
        hash::Hash,
        pubkey::Pubkey,
    },
    std::{collections::HashMap, sync::atomic::Ordering},
};
#[derive(Debug, PartialEq)]
pub struct ExpectedRentCollection {
    partition_from_pubkey: PartitionIndex,
    epoch_of_max_storage_slot: Epoch,
    partition_index_from_max_slot: PartitionIndex,
    first_slot_in_max_epoch: Slot,
    // these are the only 2 fields used by downstream calculations at the moment.
    // the rest are here for debugging
    expected_rent_collection_slot_max_epoch: Slot,
    rent_epoch: Epoch,
}

/*
A goal is to skip rewrites to improve performance.
Reasons this file exists:
A. When we encounter skipped-rewrite account data as part of a load, we may need to update rent_epoch.
B. When we encounter skipped-rewrite account data during accounts hash calc, the saved hash will be incorrect if we skipped a rewrite.
     We need slot and rent_epoch to recalculate a new hash.

cases of rent collection:

setup assumptions:
pubkey = abc
slots_per_epoch = 432,000
pubkey_partition_index of 'abc' = 80

So, rent will be collected or a rewrite is expected to occur:
 each time a slot's pubkey_partition is == [footnote1] pubkey_partition_index within an epoch. [footnote2]

 If we skip rewrites, then pubkey's account data will not be rewritten when its rent collecion partition index occurs.
 However, later we need to get a hash value for the most recent update to this account.
 That leads us to the purpose of this file.
 To calculate a hash for account data, we need to know:
   1. the slot the account was written in
   2. the rent_epoch field of the account, telling us which epoch is the next time we should evaluate rent on this account
 If we did not perform a rewrite, then the account in the append vec it was last written in has:
   1. The wrong slot, since the append vec is not associated with the slot that the rewrite would have occurred.
   2. The wrong rent_epoch since the account was not rewritten with a newer rent_epoch [footnote3]

Reason A for this file's existence:
When we encounter the skipped-rewrite account data as part of a load, we may need to update rent_epoch.
Many operations work like this:
  1. read account
  2. add lamports (ex: reward was paid)
  3. write account
If the account is written with the WRONG rent_epoch field, then we will store an 'incorrect' rent_epoch field and the hash will be incorrect.
So, at (1. read account), we must FIX the rent_epoch to be as it would be if the rewrite would have occurred.

Reason B for this file's existence:
When we encounter the skipped-rewrite account data during accounts hash calc, the saved hash will be incorrect if we skipped a rewrite.
We must compute the correct rent_epoch and slot and recompute the hash that we would have gotten if we would have done the rewrite.

Both reasons result in the need for the same answer.
Given
1. pubkey
2. pubkey's account data
3. the slot the data was loaded from (storage_slot)
4. the highest_root caller cares about

We also need a RentCollector and EpochSchedule for the highest root the caller cares about.
With these:
1. can calculate partition_index of
1.a. highest_root (slot)
1.b. storage_slot

We also need :
fn find_unskipped_slot(slot) -> root
which can return the lowest root >= slot - 1 (this is why 'historical_roots' is necessary) Also see [footnote1].

Given these inputs, we can determine the correct slot and rent_epoch where we SHOULD have found this account's data and also compute the hash that we SHOULD have stored at that slot.
Note that a slot could be (-432k..infinite) slots and (-1..infinite) epochs relative to the expected rent collection slot.
Examples:
1.
storage_slot: 1
highest_root: 1
since pubkey_partition_index is 80 and hasn't been reached, the current account's data is CORRECT
Every highest_root < 80 is this same result.

2.
storage_slot: 1
highest_root: 79
since pubkey_partition_index is 80 and hasn't been reached, the current account's data is CORRECT

3.
storage_slot: 1  (partition index = 1)
highest_root: 80 (partition index = 80)
since pubkey_partition_index is 80 and it HAS been reached, but the account data is from slot 1, then the account's data is INcorrect
the account's hash is incorrect and needs to be recalculated as of slot 80
rent_epoch will be 0
Every highest_root >= 80 and < 432k + 80 is this same result

4.
storage_slot: 1  (partition index = 1)
find_unskipped_slot(80) returns 81 since slot 80 was SKIPPED
highest_root: 81 (partition index = 81)
since pubkey_partition_index is 80 and it HAS been reached, but the account data is from slot 1, then the account's data is INcorrect
the account's hash is incorrect and needs to be recalculated as of slot 81 (note 81 because 80 was skipped)
rent_epoch will be 0
Every highest_root >= 80 and < 432k + 80 is this same result

5.
storage_slot:        1  (partition index = 1) (epoch 0)
highest_root: 432k + 1  (partition index = 1) (epoch 1)
since pubkey_partition_index is 80 and it HAS been reached, but the account data is from slot 1, then the account's data is INcorrect
the account's hash is incorrect and needs to be recalculated as of slot 80 in epoch 0
partition_index 80 has NOT been reached in epoch 1
so, rent_epoch will be 0
Every highest_root >= 80 and < 432k + 80 is this same result

6.
storage_slot:         1  (partition index =  1) (epoch 0)
highest_root: 432k + 80  (partition index = 80) (epoch 1)
since pubkey_partition_index is 80 and it HAS been reached, but the account data is from slot 1, then the account's data is INcorrect
the account's hash is incorrect and needs to be recalculated as of slot 432k + 80 in epoch 1
partition_index 80 HAS been reached in epoch 1
so, rent_epoch will be 1
Every highest_root >= 432k + 80 and < 432k * 2 + 80 is this same result

7.
storage_slot:         1  (partition index =  1) (epoch 0)
find_unskipped_slot(432k + 80) returns 432k + 81 since slot 432k + 80 was SKIPPED
highest_root: 432k + 81  (partition index = 81) (epoch 1)
since pubkey_partition_index is 80 and it HAS been reached, but the account data is from slot 1, then the account's data is INcorrect
the account's hash is incorrect and needs to be recalculated as of slot 432k + 81 in epoch 1 (slot 432k + 80 was skipped)
partition_index 80 HAS been reached in epoch 1
so, rent_epoch will be 1
Every highest_root >= 432k + 81 and < 432k * 2 + 80 is this same result

8.
storage_slot:            1  (partition index = 1) (epoch 0)
highest_root: 432k * 2 + 1  (partition index = 1) (epoch 2)
since pubkey_partition_index is 80 and it HAS been reached, but the account data is from slot 1, then the account's data is INcorrect
the account's hash is incorrect and needs to be recalculated as of slot 432k + 80 in epoch 1
partition_index 80 has NOT been reached in epoch 2
so, rent_epoch will 1
Every highest_root >= 432k + 80 and < 432k * 2 + 80 is this same result

9.
storage_slot:             1  (partition index =  1) (epoch 0)
highest_root: 432k * 2 + 80  (partition index = 80) (epoch 2)
since pubkey_partition_index is 80 and it HAS been reached, but the account data is from slot 1, then the account's data is INcorrect
the account's hash is incorrect and needs to be recalculated as of slot 432k * 2 + 80 in epoch 2
partition_index 80 HAS been reached in epoch 2
so, rent_epoch will be 2
Every highest_root >= 432k * 2 + 80 and < 432k * 3 + 80 is this same result

[footnote1:]
  "each time a slot's pubkey_partition is == [footnote1] pubkey_partition_index within an epoch."
  Due to skipped slots, this is not true.
  In reality, it is:
    the first slot where a slot's pubkey_partition >= pubkey_partition_index - 1.
  So, simply, if the pubkey_partition for our rent is 80, then that slot occurs at these slot #s:
    Slot:...........     Epoch:
    0           + 80 for epoch 0
    432,000     + 80 for epoch 1
    432,000 * 2 + 80 for epoch 2
    ...
  However, sometimes we skip slots. So, just considering epoch 0:
    Normal, not skipping any slots:
      slot 78 is a root
      slot 79 is a root
      slot 80 is a root (account is rewritten/rent collected here)
    Scenario 1:
      slot 78 is a root
      slot 79 is skipped/not a root
      slot 80 is a root (account is rewritten/rent collected here along with accounts from slot 79)
    Scenario 2:
      slot 78 is a root
      slot 79 is skipped/not a root
      slot 80 is skipped/not a root
      slot 81 is a root (account is rewritten/rent collected here because of slot 80, along with accounts from slots 79 and 81)
    This gets us to looking for:
      the first slot where a slot's pubkey_partition >= pubkey_partition_index - 1.

[footnote2:]
  Describing partition_index within an epoch:
  example:
    slot=0       is epoch=0, partition_index=0
    slot=1       is epoch=0, partition_index=1
    slot=431,999 is epoch=0, partition_index=431,999
    slot=432,000 is epoch=1, partition_index=432,000
    This is NOT technically accurate because of first_normal_slot, but we'll ignore that.
    EpochSchedule::get_epoch_and_slot_index(slot) calculates epoch and partition_index from slot.

[footnote3:]
  when we do a rewrite of account data, only this data changes:
  1. rent_epoch
  2. computed hash value (which is a function of (data, lamports, executable, owner, rent_epoch, pubkey) + slot
  3. into a new append vec that is associated with the slot#

*/

lazy_static! {
    /// The default path to the CLI configuration file.
    ///
    /// This is a [lazy_static] of `Option<String>`, the value of which is
    ///
    /// > `~/.config/solana/cli/config.yml`
    ///
    /// It will only be `None` if it is unable to identify the user's home
    /// directory, which should not happen under typical OS environments.
    ///
    /// [lazy_static]: https://docs.rs/lazy_static
    pub static ref interesting: HashMap<Pubkey, usize> = {
        use std::str::FromStr;
                let i2 =
        vec![
        Pubkey::from_str("5PjLaJCTHRzLoAfKY2fvHMqcABHV4vGbnyJ8qoAtakc6").unwrap(),
        Pubkey::from_str("5PjMZVHXNJM7ewASiXLrQUTkNzCk6DQEZoooaKP1Zreq").unwrap(),
        Pubkey::from_str("GRi5d3jn7isiL44kwrrqCmHsjuajtzugie35FF51ghmd").unwrap(),
        Pubkey::from_str("5PjJqVoGCDrAGchEM24xJX2fu53dfh7qQxbNCTFLyMTi").unwrap(),
        Pubkey::from_str("5PjGB9Zkx49hmFWSczGaLu7AJHTFCksT7eUNa3ykpVsE").unwrap(),
        Pubkey::from_str("5PjJjUfSgv37grxiXHpwFvYRhPxVqLpnmxGP29sGpuNm").unwrap(),
        Pubkey::from_str("5PjGc5ih6oCLmaq3PbQriEYdBni3jdH4XPgF4fn4xjRp").unwrap(),
        Pubkey::from_str("BByybB14PtpKy1n7SCVrXXKXYYCnahLrdxHKsF8gigPJ").unwrap(),
        Pubkey::from_str("xCcPEqqcAKr6zuqmha4rksv9oMb18jrhd3d93P7vFUo").unwrap(),
        Pubkey::from_str("5PjKoYrRXzuWB1T9FVz5LZsqmjPBmTiJU5rBY2Y3VBrf").unwrap(),
        Pubkey::from_str("5PjKvvF7epCYq9zWNq6TtmdgHhiDHpeNDEcQFsR8wsjv").unwrap(),
        Pubkey::from_str("5PjHMpmDr3m96J5VzLKkcDTCHnKscw61vyzAzGiTWx7F").unwrap(),
        Pubkey::from_str("5PjPac5kxe3PWbuTXsmEnrLg4Kr9cpSwh36J7HoUbofb").unwrap(),
        Pubkey::from_str("5PjKMzVXJ3rVXiByGrdhjr2xRfiVGcYcJ9yJ49ne1LSC").unwrap(),
        Pubkey::from_str("5PjPJRSGnB5P9xprYtLqZ78uymnPS9V6WJq96GgUHgqo").unwrap(),
        Pubkey::from_str("5PjLsSec3JGrhxtzQ8f4GQZL6VoTxENyyqbDsr1u4daK").unwrap(),
        Pubkey::from_str("ES1nh3hdKbMbZryDmok7bMWne8ZFygee1FxuybYExecT").unwrap(),
        Pubkey::from_str("5PjL1nTexaNAFUpNU6N44agtSTvZfs4hUmPJLLdHfkq6").unwrap(),
        Pubkey::from_str("5PjMNLT3WyGfq36F1MdUfkefhviysW9kCLEdypxAA7WQ").unwrap(),
        Pubkey::from_str("5PjMKaTK7eeM4nmzL2DDcy6rMpgty5ycs6kNjU4KCMph").unwrap(),
        Pubkey::from_str("5PjL31C3E9WyfwzDHHnSC27jvUUSVLqAFoqKNbqpWqWm").unwrap(),
        Pubkey::from_str("5PjM9w7eskBuzjRUS9dhdp8tBHjKjDEnDdSwhba1aDMM").unwrap(),
        Pubkey::from_str("5PjJHcgD1VhBrxw4bLMhe9EyrGcokrn6GG9TcSnD5v69").unwrap(),
        Pubkey::from_str("5PjJzcG7EKtkHhPxD7aFDSJpHLARZL4m2gcNDqJVRcbW").unwrap(),
        Pubkey::from_str("5PjPSQpFQS8xiwxpQomdGXzCGi51Y6d9ottg65KnSKS3").unwrap(),
        Pubkey::from_str("5PjK991Ae1xEeipXvYgseAXMEm74kVn3H7RTuWXF5JJX").unwrap(),
        Pubkey::from_str("5PjKFNJmsPpezoSfVbVZbYwRa1i2F1PsQ7VAu3BXR3hr").unwrap(),
        Pubkey::from_str("5PjN97aoApJmq6agkDnwFEHfkXxdvZfAv4BcegwrqJCL").unwrap(),
        Pubkey::from_str("5PjJF6E46BrMoLiNfQd55xKjPAhTMQFEEUA1dzkQ72U2").unwrap(),
        Pubkey::from_str("5PjH7A4E8U8xTwazR8KJAKpRipr36ZPYJaYVPW1VQGdD").unwrap(),
        Pubkey::from_str("5PjNvrWTQJf2vGGrHwNjSYEk5K1YwawhUHELZWhwQRFa").unwrap(),
        Pubkey::from_str("5PjHiSjn6YfqPJDhd3rox3Uo21jdMFAgt53z6cfDExTe").unwrap(),
        Pubkey::from_str("5PjNZzg5q6oFF2k3AwnUYPDhEdM2ayBnvHMPWqqcaZD5").unwrap(),
        Pubkey::from_str("5PjNogCRjMYxPghCkZuMXkj8118Hiy8npafmi8obh79d").unwrap(),
        Pubkey::from_str("5PjLoDAXbwtA6RcMzq1ZBPNeqbWmLzaAouAKk9EqMFr3").unwrap(),
        Pubkey::from_str("5PjGuCNouD3AFgXhAdQxSf7s8igAC6XqktuD5UkZiyDK").unwrap(),
        Pubkey::from_str("5PjGs7WLavK5X213ELviTk6WV3XohTTntsQU9MgpocVd").unwrap(),
        Pubkey::from_str("5PjLpL6qZW9La6gibQCpprfea93bwyXqh5R9W7oQZP22").unwrap(),
        Pubkey::from_str("5PjMQgMgyrCmQi6jth9ZCRMGsgHzkyv7mw4SipWRiR9D").unwrap(),
        Pubkey::from_str("5PjLtN6i8for74Ag8FeX6BrtvDH5NDodPRJd8y67eWuR").unwrap(),
        Pubkey::from_str("5PjMQ48nALtxkQjCaDJwrDPydpXd2y9xUncWe8g3RZMj").unwrap(),
        Pubkey::from_str("5PjLsydEiUBSsoEwxw2hFnCBaPWnF8uWmjEy9JbE3BTj").unwrap(),
        Pubkey::from_str("5PjNdpQ5pBiX1F4EvruenxDu3RyX31o17CyHYtecd9Kd").unwrap(),
        Pubkey::from_str("5PjGnTFoutJggTbvH15k4D16JSgZ7QZwap7CvEiydFcG").unwrap(),
        Pubkey::from_str("5PjGK6Lp1BFSfNKFNu8juMN3d8Uu2DZPYRWMmGMRK8C9").unwrap(),
        Pubkey::from_str("5PjJNVjbFZce1BqiYJJwvf1ti1mqMwzzb6vt5QNhWcJJ").unwrap(),
        Pubkey::from_str("5PjM8osCU2irSUmTrNiEwnjqywnDF5cbdJkbTXVtw52u").unwrap(),
        Pubkey::from_str("5PjPnQM2vk3VWXmVPhVagWEsyvzU2CLLhEcs2fa8hbMc").unwrap(),
        Pubkey::from_str("5PjPioH9ETxZG3UUM6tgM94EmhEuhFk8y4AeCi6jDmja").unwrap(),
        Pubkey::from_str("9y3QYM5mcaB8tU7oXRzAQnzHVa75P8riDuPievLp64cY").unwrap(),
        Pubkey::from_str("B16JMAgpR84Dr6rucq4GYLZV7pdk1uPF533P9KVwNUq4").unwrap(),
        Pubkey::from_str("GXp68Kbc9xS2KnDYPJD6kVaAbsZP3xCaHfeXc6hWreJo").unwrap(),
        Pubkey::from_str("5PjHxcjRfA5KDA8Heuuq3rahQXPwK3qpJ9fFjU7LtQUm").unwrap(),
        Pubkey::from_str("5PjMgYnuX9U2wcbSoPE5WUyjQ8RgBA98TvdaP5MwRMe1").unwrap(),
        Pubkey::from_str("5PjL2ut3tjxfnPNdYE1yNZcsY9jhnBU9peFb4az2SLAd").unwrap(),
        Pubkey::from_str("5PjJXpn1gy9KDMZEXFZpYeoi1VJ3djgSZf3z27hnXtCF").unwrap(),
        Pubkey::from_str("5PjKNrcRLkrSn4sB5QNpTkv3GfQaQuoqSihxB5T6vLYL").unwrap(),
        Pubkey::from_str("5PjNFoXKX47T1Ai98Y3hF2jxCzUfbjMdX14GRcTEDzme").unwrap(),
        Pubkey::from_str("5PjJ3npf3v8vTo6nQ2FCF2Rbw74P3pxBTs8bciLzDo61").unwrap(),
        Pubkey::from_str("5PjGN9PspjeuUtHphpdwPkT377EvcRqXH6t9Kci7bku3").unwrap(),
        Pubkey::from_str("5PjHpDWALMa5QqUNeMi5J24PKuSGTncRQkWNeMjnj6Zi").unwrap(),
        Pubkey::from_str("5PjGpVYdiojcMyTucQBk9prD2VBq7GJMcsMegXNzKvN2").unwrap(),
        Pubkey::from_str("5PjNrZZbQX7Jf3ovMif7YmsdDnmcApUqZzwh5ocQwBCv").unwrap(),
        Pubkey::from_str("5PjJ5n77jRJc6izAGS65fXY47pJuqkWzGxCkAPhqoVU1").unwrap(),
        Pubkey::from_str("5PjNjZBJ7mWbu6FhLNyWeLTwCNchSwP3gQhGPrjkpWru").unwrap(),
        Pubkey::from_str("5PjHPpnryGSWbLajknAz2QFN1Dz5akixtPDrjfGAhef5").unwrap(),
        Pubkey::from_str("5PjHZiUnMPbqTcpweFCJNci86mDPDxRNMgZTFEpBNaqG").unwrap(),
        Pubkey::from_str("5PjMnnKjjTZjux8QcqtmuCAt7r6pEL6DKEWB3P99E39a").unwrap(),
        Pubkey::from_str("5PjHbuqZz3taQhpL4EyBiX1zJ3h1efKbFPLWtFyi2nW1").unwrap(),
        Pubkey::from_str("5PjP3esgJGuhcR3xSHjG5RNqJrYCqk7HnRGRrYmukNvS").unwrap(),
        Pubkey::from_str("5PjNZz3CowBB6K7wJvHjLaADhpuqFbZtfpRYWpQjjeDD").unwrap(),
        Pubkey::from_str("5PjKqqeH4xLRoKJn21S5uttZqnhE6uoL24nEvDLGB9y9").unwrap(),
        Pubkey::from_str("5PjPKAau76r3MSbgNDp1UpBeh2zp8vDwxVnvNzh7kFEp").unwrap(),
        Pubkey::from_str("5PjLnodYAzAKGuv4jis74HEhXuGkug5uppXGdUEhLS2Y").unwrap(),
        Pubkey::from_str("5PjPnmeqVFDsCkJ2ZaYxF4a4A4LapUrahk36rPTiy8fg").unwrap(),
        Pubkey::from_str("8tyCKXSjk8cm9bvNVhwEj62HizKPCBYNHBp4Z3hVRkFd").unwrap(),
        Pubkey::from_str("DojkWxTMc3NQkeKQ7Tg8c5uf5eZXBiQWxkyVahGQHtcZ").unwrap(),
        Pubkey::from_str("5PjJN6ec9c718JuFYkeTQE3DAR5aTAEoaJHPsQP6diZe").unwrap(),
        Pubkey::from_str("5PjP95AUFuYHtcAyMQCSw6j5WndV9FwtQy8gQ7qwkkaR").unwrap(),
        Pubkey::from_str("5PjNgmZCtehJQt95bCgSqnCShv8hQvUcsiNEMdDYm1Ne").unwrap(),
        Pubkey::from_str("5PjHmPWhht1mMeksgdeSow2yxAMW8hstkoYpUWSngD7q").unwrap(),
        Pubkey::from_str("5PjGDpgtjWAky1HC62AFX2kfPr55dCkWsMzsJANTNSgz").unwrap(),
        Pubkey::from_str("5PjHy3BBHZzPnv5LGhtfPWkyvYDkcNi5tCdWhpazMKk9").unwrap(),
        Pubkey::from_str("AzZrnBurWzCNLHBKNfgXMFgmcjvm3RDWU75qaoFKLX9z").unwrap(),
        Pubkey::from_str("JBMtUoKUSDTLHffvJmPg4pF36KjUaa5q6xEjkHsnBHxs").unwrap(),
        Pubkey::from_str("5PjGoWi1GnpiAZwCwtjRMG5CB8Lo2PyQYB6a98z29K4T").unwrap(),
        Pubkey::from_str("5PjK3CasjAepZ9ZQoo4RE6uspTNNymLd38jc1kQXWLn6").unwrap(),
        Pubkey::from_str("5PjKPF1kVBpRePQsTpa8Hs7jNDgg4AzEyd8tc9XnM7Bt").unwrap(),
        Pubkey::from_str("5PjKZ68aQkBRN8uFVssGqh8HhFkxeDDANt61P56dA8HN").unwrap(),
        Pubkey::from_str("5PjK57ENvHCkWQGdUtvUvMSCz7Sx13nGRQwfdo5zBts9").unwrap(),
        Pubkey::from_str("5PjJyH8kQ5UgVMYpFf6fhLG1j7CnrnVC6Z9m1PVLd6GF").unwrap(),
        Pubkey::from_str("8kzLcBCEC2xSXvGGG8jHjkBciqw3bPR1VwA1pCCPCpc9").unwrap(),
        Pubkey::from_str("5PjJ9phfBYnYmPpPWhGfu9Chpygm62pZocJEDa3siBYS").unwrap(),
        Pubkey::from_str("5PjKYD8RZ9pPT7PMfgYZPLBviMHj32jGUqMeJ4yHvkgZ").unwrap(),
        Pubkey::from_str("5PjP1rNMbjCsGSxnwYdVdkGkgM7yDK43CHud1PQ5zn3e").unwrap(),
        Pubkey::from_str("5PjKGF6kCGDzhqn5yJRcSttjQ3hULugjXecLSvcXYCfz").unwrap(),
        Pubkey::from_str("5PjMkwtQjw9LE5wqMjYo1TRsF4eZmC6oHYcrBzJ2qmQe").unwrap(),
        Pubkey::from_str("5PjPUfrMJhD6dAsMAjwJZkSfWjV7KXWr4PPuhy6jpzgX").unwrap(),
        Pubkey::from_str("5PjM1JipSrnwqAho635GYq9YaeJ2boNM3Sb5i5gJjoV9").unwrap(),
        Pubkey::from_str("5PjGW4cRa9jae6My4bUsxKfZ4hR7ZCLkoAycz4yu9Dmk").unwrap(),
        Pubkey::from_str("5PjLJUUsGnmpcqXnTXAk82wCPbuDAUL58muBpTDkciYu").unwrap(),
        Pubkey::from_str("5PjM5UdjoiF7RL5ofuSoMkfTaADiVtdt63o3uct7TVtQ").unwrap(),
        Pubkey::from_str("5PjH5q6dWsaDcEThFJuE5tFvHVE72rWws4mSD94mzzo1").unwrap(),
        Pubkey::from_str("5PjHErw7PdGnf2UwFoeH39dGqw9Lv6m4zgDoDU6TH9bE").unwrap(),
        Pubkey::from_str("5PjMmW7FcbgaK1N1w9ofbs8TDBPAuLBKJCCR28REMSoM").unwrap(),
        Pubkey::from_str("5PjNBH4LvxK47H25fc9V5wiPXKqRtqNL11u99krCGKjx").unwrap(),
        Pubkey::from_str("5PjNkM6REXFbdiTP3GHHtVVE54udwct4TZRebr9ftqW3").unwrap(),
        Pubkey::from_str("5PjKkbewV59rfJtK8XZSQme6DDb6FTfc1z1t9wQ7tNGx").unwrap(),
        Pubkey::from_str("5PjPnnzMCdNuZnNF4JfhdYypWQmLERxxeDMD9Sqs77Sm").unwrap(),
        Pubkey::from_str("5PjKpTv47U3UdNGZGUkVokePCL2DQu7yoyXnruKFLw49").unwrap(),
        Pubkey::from_str("5PjP7oJXoT7mxxEDepFJHkYSr5iYc2pgHpFgLbMgyu18").unwrap(),
        Pubkey::from_str("8hqZS26KSh4pR2VWDBc1qS9rTs7XktN3vTHWVGCDVuYu").unwrap(),
        Pubkey::from_str("5PjKLf5pPFeayZEe9FNhTWaqab2fdE1XjQxiCaqsHoEt").unwrap(),
        Pubkey::from_str("5PjLtKMmdU5Bq9Husg9i571R94NANBVhcYN7EmzwbYKV").unwrap(),
        Pubkey::from_str("5PjKq6sCg7F1Xh3wdQZtXJNH9X8bMSGGbELZiUezxafC").unwrap(),
        Pubkey::from_str("EvtMzreDMq1U8ytV5fEmfoWNfPhrjZ87za835GuRvZCc").unwrap(),
        Pubkey::from_str("A1enLcj9XmuVeYCQScEruwnfAz7ksQhbuGFUgvgeS1a6").unwrap(),
        Pubkey::from_str("5PjPY1FdqV4F3Xb8dKKb5yKdeNcE5y4GtnEfNvDWhR2d").unwrap(),
        Pubkey::from_str("5PjKzgewdhnFHtYVdTvFKUnrj59j17raPxwYN9TAY9Hi").unwrap(),
        Pubkey::from_str("5PjGMDxc9yossMRutmHwG271YiMAm96mdvT5koSvEHxL").unwrap(),
        Pubkey::from_str("5PjMzeZCLNGUyQzpowRRr7FjLZawhWL1vgugpbDVNdbs").unwrap(),
        Pubkey::from_str("5PjGtSUEzeybZ3iY9EVJ2rJjkSVm5sHYg9BuH2uPF7Ts").unwrap(),
        Pubkey::from_str("5PjGhQ2o3r9c35yABQknSxAuHbzWchdz5brBX33UTwrP").unwrap(),
        Pubkey::from_str("5PjMMEeUd43cEMdkkoMs9Y66bGbK8wykJXGCbBy2HXLY").unwrap(),
        Pubkey::from_str("Evwz4AatByRhePuZDe1zVt8Uau9ke6PWm4XNQczFdfTB").unwrap(),
        Pubkey::from_str("5PjKDXTQzAV1CRuQ35uiBubKwjB6tLftcaZDr3xaLPvz").unwrap(),
        Pubkey::from_str("5PjKbN1Sz7BEpzFZMmTHUmkHiuDNtb6HxATPerHGGqS8").unwrap(),
        Pubkey::from_str("5PjLd53HaGdd8U7p9w8faT2XQPTRtoY2zA8YTsw4hiHJ").unwrap(),
        Pubkey::from_str("5PjNeBznTkG34dUBN6bQL3syg7m5ifHv55bfLSsHmUUB").unwrap(),
        Pubkey::from_str("5PjH8ebDZyh4dUnAV8C6mu2YUFHkxwgBFkRMs45LbfE6").unwrap(),
        Pubkey::from_str("ACahZ8AwuxqAkK1sWgMbqRHPhrnMGErYe6vXy7V3886E").unwrap(),
        Pubkey::from_str("5PjKrJpsABRBEXgBgAiqVP597uDNt2YpdQkEd4wjeycA").unwrap(),
        Pubkey::from_str("4F6mgqT7RBCeTNMbHGgSEed7o4waRrZjXj4erE1T34i8").unwrap(),
        Pubkey::from_str("5PjPbzqDajrQAL6oUjQTMSLUjsrcKud9n5u7pKXC5urG").unwrap(),
        Pubkey::from_str("5PjGnRA4gAhDqwpQEfPXA9NxDoEjZEzrSBrBS8KGLeMd").unwrap(),
        Pubkey::from_str("5PjMgq3ghDAu6ZxNPbpKUxyxmJvLfjZAtnMJhe4NQCnr").unwrap(),
        Pubkey::from_str("5PjJ9hUF3iCXYRFF3cbq5FNKc77acDYEseWuhVkfwFCS").unwrap(),
        Pubkey::from_str("5PjKj9wF3oKWAsTr7Cfvu6gQbQEVEFMVHCawMvAAoECp").unwrap(),
        Pubkey::from_str("8G8ZrLBTH2ENUwmRFjdM19xL8nRZvbKMMw6q4YFQoK9K").unwrap(),
        Pubkey::from_str("GANbyXQSXz472yHhB2VR863d222XmKSiaJUJU36rYMV6").unwrap(),
        Pubkey::from_str("5PjGnMhB98ET6j33uaXtUaMnL1GTnUef3UceemHv5g4u").unwrap(),
        Pubkey::from_str("5PjK9b5QQ4Wxh2iwibYjf3HEEWtwzuF3jcq5nNiUqX8n").unwrap(),
        Pubkey::from_str("5PjHNnUZdcpJZdjsCNRKZrj6R3sWNs6j4y7WKZ1CXKzB").unwrap(),
        Pubkey::from_str("5PjPK847uHwqHF8boAdnDHP36aJ3WraCMw5WPEWx9dkg").unwrap(),
        Pubkey::from_str("5PjPStQqLGBZKq5KNVdY7fxJx9EwzHvnrjdEWDsDgb5s").unwrap(),
        Pubkey::from_str("5PjPbfXXFCXe8q1Ln8RPkKtTYUbkAz3nGgDcEWuw96dD").unwrap(),
        Pubkey::from_str("5PjNmo8FEk1BQuRAU6aXAgZn6pqtSsvMB86a6md9ZyTC").unwrap(),
        Pubkey::from_str("5PjJiQfgfzBwZE5CnGPV2eLt3LLNjgg4XTW96Zx9vrDi").unwrap(),
        Pubkey::from_str("5PjLaNPmGkS9nDPWtNtQJ88XHVnsKwpeztVjMTU1k9aq").unwrap(),
        Pubkey::from_str("5r3vDsNTGXXb9cGQfqyNuYD2bjhRPymGJBfDmKosR9Ev").unwrap(),
        Pubkey::from_str("5PjNpsfjMwX5RiSBGSWPbKftdGf3GkEGwdwtXysHWgkK").unwrap(),
        Pubkey::from_str("5PjPLWo61yxgbARmjLXs319EqtV4KPZ2FwprjxbGj7YM").unwrap(),
        Pubkey::from_str("5PjPRUWCxCcYv3jt2RFoquKB2jQAjijpJ7obnSHdn6pj").unwrap(),
        Pubkey::from_str("5PjKjaLtByUQsKZYgpZsv9LgieNo55pbaMxXj2HCq4ug").unwrap(),
        Pubkey::from_str("5PjHXCrxQecY9tvE4LGUg8G1VNqpJsZRjDMfnmfNVJ7e").unwrap(),
        Pubkey::from_str("5PjM3egQxPiAYCt1kj78ALugzkuL81a6j8j2LcqgnP6z").unwrap(),
        Pubkey::from_str("5PjHz6gjQnXkKLWmmxYYEmPcgf8iVadgdftsnenf91wC").unwrap(),
        Pubkey::from_str("3QaNhP4vT6PG3eoQwg2DRbH9ecmy7pR2f1PBPWCwDBYd").unwrap(),
        Pubkey::from_str("5PjMN6KHmceRTthyhvPMKpzAFpEJdNQDawti3dAZ7iDp").unwrap(),
        Pubkey::from_str("5PjKFTJp7iZGx6vnWrNLs2SueBnvhgVzBLQzyG6WUnkc").unwrap(),
        Pubkey::from_str("5PjJZkpSNTLhyLFRN8PnjxPPHGicoxM3DCnTGUmgrios").unwrap(),
        Pubkey::from_str("5PjGhfJHHSUxJkggKeUiKsA4FhRECBgsczeTqMhftGdp").unwrap(),
        Pubkey::from_str("5PjPHuLo8RU5pC6qT1wNPRM5W4niGmQeaYDVxhrwpBW5").unwrap(),
        Pubkey::from_str("8ER3VKcuysXDoKTgGQ5XQoxWg9RJHzdmSGgGWMbBTPS1").unwrap(),
        Pubkey::from_str("5PjK4tVbBzfagcv1GJEPHB7RytC2hxtsnDJ6QC4YfqDZ").unwrap(),
        Pubkey::from_str("5PjLvEggQzrHaAn9dKDgrSQVomurYX9Ax79WrQ5wi9x7").unwrap(),
        Pubkey::from_str("5PjLaVzgMPCWzRGn3MYvSWfMLEmG1MBd5pyaQEndT9e2").unwrap(),
        Pubkey::from_str("5PjLwZGm9dkJ3tw1FHjoTbBsqNEfjgVgf8VHvmfrWmT5").unwrap(),
        Pubkey::from_str("5PjPWtz3NbraFT8BQXBXRAD3W3XWbfjZ56wMbnGk83KB").unwrap(),
        Pubkey::from_str("5PjNaRX5FMkXWpHW76374vnWDMjCNFYXdj4rbrwskpZf").unwrap(),
        Pubkey::from_str("5PjGkZKF1zbFqEgDN25dHTR4UFosKfvjnPeEYKzmPSST").unwrap(),
        Pubkey::from_str("5PjGwvv6tx6T7DoQTKpgVahbFJAzhownS5BoxuFUqcQH").unwrap(),
        Pubkey::from_str("5PjNB1ecWHicPLDeGcxsSdzcgZJFScxZCPaatPAmBvqW").unwrap(),
        Pubkey::from_str("5PjH75C6cMTevcipsRChsmHqg4X8VZvRDK8U4Ux31uj7").unwrap(),
        Pubkey::from_str("5PjPfZ9eUSGRmcRuMGs6kZJvbKX81ts42F1ByJEK4W7x").unwrap(),
        Pubkey::from_str("5PjJXwTtfsKbTXjWtMpwq6czs1RRJZWP8hyVofMQcC6j").unwrap(),
        Pubkey::from_str("5PjJ8gBBupR8qifYTLUuCqd1eg3stUD8AApep3zBAsdJ").unwrap(),
        Pubkey::from_str("5PjLKJ7ETzW6MLz9o8i2mMvuvEJu3Fcp6YjpuMrTvZ58").unwrap(),
        Pubkey::from_str("5PjJa1PVtRpDadSRaD8pNFQcYeSeV33RGeFzY7vA68Rt").unwrap(),
        Pubkey::from_str("CUJorkzjpvZnm7ynSaxuwtK2MJk21ReFDSF4uNc3xrsc").unwrap(),
        Pubkey::from_str("5PjJvEkBnp612n9npeNjN1oetYp8TwuecAW4nv1xw7Sw").unwrap(),
        Pubkey::from_str("5PjMuymRQqoB4wjRYrL8agcoZgyYoeGhAawFsi78y618").unwrap(),
        Pubkey::from_str("5PjPfsochd665JhwxsWJeRGiQ2nihfywFmc9npHC36Nw").unwrap(),
        Pubkey::from_str("5PjJ1YmYSn4pyfG5TnjRhVN5eGmPFV8BZnv5ewQWETqW").unwrap(),
        Pubkey::from_str("CdbgqE5B9oADrSAWc51Mgw6c3B6nvYJ4c431rftpoVqZ").unwrap(),
        Pubkey::from_str("5PjP7DLTA1jTyYT3txUFc5dKvUiMsBRFQozSeP2AHRRv").unwrap(),
        Pubkey::from_str("5PjNzADACZgj1PtxH5qwDxBwTkDjiFF5g1PgNtcnXWab").unwrap(),
        Pubkey::from_str("5PjKePeXJpcASvrjp76KL9ToRCJYobxAKWD7jFEqsPdN").unwrap(),
        Pubkey::from_str("5PjHAPPyr81Q6X6U25eK1RjnonypBnkHB5jnzVHWbAPD").unwrap(),
        Pubkey::from_str("5PjGtKYqAjyGvSdrzvom3JbikVgtatVmnpdDF3bi79xk").unwrap(),
        Pubkey::from_str("5PjP9fw4FGFkpihFLgw65zELYPakhHTadKZPkWzrJBw3").unwrap(),
        Pubkey::from_str("5PjJ8PM4jwDy852i9t2mQhDXwgt9Ewe6Mj7fXP3AdFAq").unwrap(),
        Pubkey::from_str("5PjJdNyjB5F7PSximCgdkHTzgar4dJEduQynGqtLDoiN").unwrap(),
        Pubkey::from_str("5PjNsoXBJSYfcV1AaUrrVTxeLW4yzt7gZjwVPbMLYu5h").unwrap(),
        Pubkey::from_str("5PjNKVU3AUKBSHumMjqwbrEBdVEZa8taE3qmt4QgqeXE").unwrap(),
        Pubkey::from_str("5PjMvs5H7jAcdH7sFLWu2Nq7UZ6Vm9bwqHSrvsnNjM1B").unwrap(),
        Pubkey::from_str("5PjJkJ3wiLnKb4WcMNUtiWNR8rrh33spCtc4thMbQu4H").unwrap(),
        Pubkey::from_str("5PjJP3xSfnnnEdTY6aCDRe3XUHWg55wHWzAPR1GJyQTT").unwrap(),
        Pubkey::from_str("5PjNR8Sa8CbMxG6KZWL8Gb38XBv2fXGYb9TSHoyvvi8R").unwrap(),
        Pubkey::from_str("5PjHkyGG9DLUYFEjg6YE9JJMKyp71g5kpUuk9i7tURkA").unwrap(),
        Pubkey::from_str("5PjGnNRYGs5YLFWk7zfGfVkM9EkJqH7iznXK9wc5QS7G").unwrap(),
        Pubkey::from_str("5PjMxaijeVVQtuEzxK2NxyJeWwUbpTsi2uXuZ653WoHu").unwrap(),
        Pubkey::from_str("5PjJKzjhqgNWYPE3PMnkHUfP8yYJeNucxL4bHPTQvsbn").unwrap(),
        Pubkey::from_str("5PjK23PobLEfFtbZCE17ZN9FynGzrv5AZGcLSWKq6SAU").unwrap(),
        Pubkey::from_str("5PjKAc3fsHGqSebwvfhNh749ae36Dke51dPN5r2bxNrU").unwrap(),
        Pubkey::from_str("5PjNKkg2G16La2g4QX2rqAs8p1TM2KUtefRtxKYwpv6g").unwrap(),
        Pubkey::from_str("5PjLTtK1wNRVpDgFA99w1B3ZPirJTWcsaKtrKuqjJUtM").unwrap(),
        Pubkey::from_str("5PjLcY2qMK8q8xuebEBaLfPjcK3QDxTCT9TaHMTD9bU6").unwrap(),
        Pubkey::from_str("5PjNKZdfoXX4ZE1n7kqe4BmqLVUdpFcFcDWTWKKPnWEQ").unwrap(),
        Pubkey::from_str("5PjHZi2D9H6Z8zqhyg4LgaXtCx563gKMimpPuruCYnQ6").unwrap(),
        Pubkey::from_str("5PjJHwRAL7ZERCRTySeHDDiNcymwgMS2JSLPaCf62yy9").unwrap(),
        Pubkey::from_str("5PjNdKMLhHdYWPtwRyBPsiRkY6P7po99Cim4j6Be1Kcz").unwrap(),
        Pubkey::from_str("5PjGMjyqzSVo6qY3nCSvTBbJgicr6LPHACYfpSqM45LW").unwrap(),
        Pubkey::from_str("5PjLjSDFsVwwjxuphu64KCyPQundRA53KJgbnWwQ1pDz").unwrap(),
        Pubkey::from_str("5PjMhnd9tQw5vpkTn9p8XGpuASz6257ygYf7dtgcLLX5").unwrap(),
        Pubkey::from_str("5PjGrNoQ1peC5kezHhdXaarG9tvmoSTdSUqckBcyiM54").unwrap(),
        Pubkey::from_str("5PjLDDXXh4mybzTd28MoLm9u6FgsNMkXctz7QtYJLsPC").unwrap(),
        Pubkey::from_str("5PjGNJPyJtMctBioXUG36yUpz27KSL55Y3u35w6xksti").unwrap(),
        Pubkey::from_str("5PjMoL7BHC9iExbpithgKvNNadhTAzMMb3GtorTKKDAB").unwrap(),
        Pubkey::from_str("5PjGvyr9ebCqHtmFXsvpqk9od4DUSCijxzi2oCnQ51Bp").unwrap(),
        Pubkey::from_str("5PjLhKhqz41r6kCS95JD3FznVP43BC1j16Z2h21LDrLp").unwrap(),
        Pubkey::from_str("5PjH5k5W5keJazAdHGiv8hzqeRwyQ2PkMH6bskYMNGHp").unwrap(),
        Pubkey::from_str("5PjJ5cKLywdDdvasiBkbeocCKdQDgpocT7REg2RVuC3E").unwrap(),
        Pubkey::from_str("5PjNZdmPQyG2pHr21TCQ8cQCFMg9fdDt32TTZ4ujusK6").unwrap(),
        Pubkey::from_str("8F6NCo1PiakW7m3eeEZvdxsjXF5bkLD3QZsTxaNg9jvv").unwrap(),
        Pubkey::from_str("Esm9CAT8FFBgJM63iDLRntKq3CjGZNEGidNEmcN4XVby").unwrap(),
        Pubkey::from_str("5PjGHKrzk8nrWZ2A2dPAV5XsWVm1VC6CrMsoFHTdRSzB").unwrap(),
        Pubkey::from_str("5PjK1WavKLEEVHG6AjuXRJ8z97rWJMuph3BXXJrKuEkD").unwrap(),
        Pubkey::from_str("5PjG8w31DyYUaHXHPdfGHEqcSpAWFdWprYp6fTpMrjyN").unwrap(),
        Pubkey::from_str("5Pj1kicH7V3EpDvxmXUMaBeLSnvy1Ac1wQTFVSKBDnHR").unwrap(),
        Pubkey::from_str("5PjK6m62EnSxp8qHUnYN5kVpR6pQvpoduV5sC2ZGRSwp").unwrap(),
        Pubkey::from_str("5PjLmEDurEUb3iZ5GdRnQqi6XBE2SJTwSWbnCiAhNb4n").unwrap(),
        Pubkey::from_str("5PjHiaubj3EivhvVaNaEtXNp2fHgsiwQtjx7ujDXuATK").unwrap(),
        Pubkey::from_str("5PjMTdmM3hUJtvzfAd8rczUJZCgvm317rQRbT95obXWe").unwrap(),
        Pubkey::from_str("5PjKSXSAhqAg4SoBFtdn9iLdYSt5ddRZbBW16sJ3xmmY").unwrap(),
        Pubkey::from_str("5PjMiGsa7LHJJhmNba8srLw6DfkSTDDQmisYtppRByyW").unwrap(),
        Pubkey::from_str("5PjGYVbM4xtiE6STAUH2WnGAjXn6UTet6JMgybRho7Np").unwrap(),
        Pubkey::from_str("5PjKa4uh7agYLjFSgRo2ENsjU8BwQmnkgk2Dcuiu5TuG").unwrap(),
        Pubkey::from_str("5PjKDvoroHTeDcLUrYcCdEByFFWadJ8AKxeY7DJdirHb").unwrap(),
        Pubkey::from_str("5PjLnbV564sUgqd59s3zN6cnJQMyKRppq5Hsdtk9pUnk").unwrap(),
        Pubkey::from_str("5PjMS6rmwCJoEHgzicov4Qyig2vynoXg5eo75gMGf565").unwrap(),
        Pubkey::from_str("5PjHh4ohn7Fji6R4Df3KR7VubUUygC8X8BUJvKXig6vj").unwrap(),
        Pubkey::from_str("5JvU7QoYiZDLoWro4kzDenYMNpvQsGTHhYhv9b8NzEpf").unwrap(),
        Pubkey::from_str("5PjJ1RcbPNKeNUMpgw4z8chV98LQmDxuvttc9d33tiWR").unwrap(),
        Pubkey::from_str("5PjHs1d8aKUx8aaDA5qqu9pBsmwrKThTpn6uERMNZYUF").unwrap(),
        Pubkey::from_str("DuHRmA6Dc9L9TsoxcfYFuuu4Gt9U89ogv61ewbhhbKRP").unwrap(),
        Pubkey::from_str("5PjGmWsYsvygM1DwSLL7rP98iUdRKZFSwHcrmMyQEubq").unwrap(),
        Pubkey::from_str("5PjLhTpUgdudeQDV1RjQyxFyBftQ4DzejhSttjbCZgiP").unwrap(),
        Pubkey::from_str("6MwFJtXNgPsqN7RAhtgNDrLCKoof7SJH8E1KqJArUCqq").unwrap(),
        Pubkey::from_str("5PjKW2a5UGcuaD784pCbXkPcrC1NWEcN2CXro26XeuMf").unwrap(),
        Pubkey::from_str("5PjHrniktpF72ZAyi4ddVrXdBmraY7VQFDVXuKgW7hfN").unwrap(),
        Pubkey::from_str("5PjMjwW8NB4kxduAzr4tDszZh8Ai3DQLMiGQAaHFczy7").unwrap(),
        Pubkey::from_str("5PjNVKw4xxEFGfLhRgcq3FLMgwARv1xkr42Pxag7aLnA").unwrap(),
        Pubkey::from_str("5PjH1sFKAjWuqu2kaP94uTrLEmmbS92K3yGRWs4pCKaJ").unwrap(),
        Pubkey::from_str("5PjG6VacBXnTSQ9cRoJU6osyFAuo1xMrsHZH3TtAZToA").unwrap(),
        Pubkey::from_str("uPrmw8dKecH2kwB4e1zuep1sahcKfvhFAza6E7uYtjC").unwrap(),
        Pubkey::from_str("5PjH249xoiTFUBM8iq8ULfgFmM8hpdUM3TUsvqAiAkKk").unwrap(),
        Pubkey::from_str("5PjHWVNUovz8y1i7eYWfCmkYDzMUQr7k7gXFAVsA6PhF").unwrap(),
        Pubkey::from_str("5PjLBuhrCBABtkCrtL1iRqCGba6dnj7PTJ9kAxoQfc1T").unwrap(),
        Pubkey::from_str("5PjNsGwoQs3LrnNytkyGCu43jqzanQWjPQhuK9ZPTLTw").unwrap(),
        Pubkey::from_str("5PjJZiXSeEQD7q6CRW2Yek9F4PovkKionwHTnRExQzUq").unwrap(),
        Pubkey::from_str("5PjMqMaof66pxXq9mzgvmPFArAJDqPiKd5bQjHAfDcYL").unwrap(),
        Pubkey::from_str("5PjK1No5Fx2SJW7ha48k1ersny2EoDquNEPdminVQmEX").unwrap(),
        Pubkey::from_str("5PjHcToXiFg3AfRh1kf46vvhHvTHn6RCECtAdWdzq5XQ").unwrap(),
        Pubkey::from_str("5PjGpdnVo6dKvcVFe8kHLdTjwgcGr5QcJLGuNgFZ1tAt").unwrap(),
        Pubkey::from_str("5PjPXHzLwaG3PkhAf1E5pkfiUa51h979jb9JcT9uSLh5").unwrap(),
        Pubkey::from_str("5PjHcv95jLbW6X1QUgv8zYNyzd5gLpwirfURN8K4KX92").unwrap(),
        Pubkey::from_str("5PjPFLUR9K1kbAnxXdrp5Bf5sRSoiGbEtDa7J4bznAY1").unwrap(),
        Pubkey::from_str("5PjPZ9e86boZGp3VNMac9Weq4zwroNt3oxTU6TBQFciS").unwrap(),
        Pubkey::from_str("XbkV9HZpLdv3CjMUfoq4t8nkxR6UguHb4oP8aAKBGV2").unwrap(),
        Pubkey::from_str("5PjLe2nuzrMqGswZYsaszd48WAv9sM7UnDaszLVMGsEB").unwrap(),
        Pubkey::from_str("5PjLXiQ1j2Tw3NLtDm8TYkB3zbQ7Hdop7K3L9L1MkE41").unwrap(),
        Pubkey::from_str("5PjP5ePMnJPFkeGNqHD1rCPKSosTrSNPGaGUctDpUr7r").unwrap(),
        Pubkey::from_str("5PjLfzPut9oSP2uJSzFToLP3deJ4P9aNuaS2pSygets7").unwrap(),
        Pubkey::from_str("5PjGB8FFt9H7rpGYPsKx4ykmediZWEg8QuJvqLZQEu16").unwrap(),
        Pubkey::from_str("5PjGWwcLiWhCMihHS2PvcTR64TPa8Lh4eWs5d4GnKnSu").unwrap(),
        Pubkey::from_str("5PjNXWRFydReLACN4uyneuqgita1DBKaAenLmJkxnvnW").unwrap(),
        Pubkey::from_str("5PjHrrZSjXztE1osswkEVRWtJ1zJnZsTrPEpDq5TXfXG").unwrap(),
        Pubkey::from_str("5PjKpdXALkbA1D38NTDnGGFtQhc16wC4Qt82n45sv8Um").unwrap(),
        Pubkey::from_str("5PjLaDSqSVNwMvWvMgEFACdMMfNqqvLYVCJ1to9stuQ7").unwrap(),
        Pubkey::from_str("5PjPZmvrqov9Xbafvq3FL54XU65hykkX29r6Rp2Rv6pV").unwrap(),
        Pubkey::from_str("5PjLyxCdU6RmunxrwM6UDHtdmjbFqZg9etNZPRdY3pCa").unwrap(),
        Pubkey::from_str("5PjMJgFFJGdJHAh9BuSkx6pzJ2TkGkRpPnAGcyPkVN6E").unwrap(),
        Pubkey::from_str("5PjMNskPkuDiSb3dEAZnAyNbRaza2ubzfkTgiW8CFPFw").unwrap(),
        Pubkey::from_str("5PjGPQG5QPrH242cpm9hHAk1BBTV5bveWk12LAhtCkY7").unwrap(),
        Pubkey::from_str("5PjPdcUgbZ9DCRBSRP2zAiCcfL9nZi2BJUquFZ3Wv6zp").unwrap(),
        Pubkey::from_str("5PjM6bEPjAkcjgKV1Dj57ZsZNruu5u1XSAtmmjApSbBJ").unwrap(),
        Pubkey::from_str("5PjKqvnD2qHqJJPVHx8Z3MfmUgXokSVESai82FadKyB6").unwrap(),
        Pubkey::from_str("5PjMeKkBRhv5rLFVuaBg8VAFkNGxi9qRBvtdun7dfvgh").unwrap(),
        Pubkey::from_str("5PjGDigNf4EM5P2TDcaLDAQBqgqgyzhEyjDESJ22h2vN").unwrap(),
        Pubkey::from_str("5PjJE4NvMJzjaQqAaFHCCweAhrV2kSvKFjcMgNWj3eGn").unwrap(),
        Pubkey::from_str("5PjJHcGwtRv6sqvhV3A7fKwZgRdRx7n88ZWDWbEu6ADt").unwrap(),
        Pubkey::from_str("5PjJZ8Dfrp956uyeaSsPVSScTWGNgVPCfbKp8CtnTu1A").unwrap(),
        Pubkey::from_str("5PjMsSLZ71twpafTzXvjmW3NhGZA1WgGhXJVpRJvbDWy").unwrap(),
        Pubkey::from_str("5PjLJx9S9dWKS42PmWcan9T7WKR9pZ5CHJXfMrvqBNE5").unwrap(),
        Pubkey::from_str("5PjLcJq9JR1PBKLkr4a1isD2BmADjX4ajncYcUAqB1XJ").unwrap(),
        Pubkey::from_str("5PjJ6nwrMgdgKUCu1zWLHiE3EHQmr2q2eMPNxSWAbyaR").unwrap(),
        Pubkey::from_str("5PjPoqXGoRqeSFeWtWFon2WGS6ivwabeSgE2wqKfoCsU").unwrap(),
        Pubkey::from_str("AqbAM68p3cx8w7hc16QAFqqwC8FLfQPPW1iKxxcjVgYy").unwrap(),
        Pubkey::from_str("5PjKJgKU4gR27DUsXfuKqyhe1jz9wwKCmj4A6LCQGfK9").unwrap(),
        Pubkey::from_str("5PjKNL52g9UWHxncFXkUFdpzLJkh6CDXWWb2aFDr9fxk").unwrap(),
        Pubkey::from_str("5PjKRs765KcCoQFyM6zcVxMkQ23WaQ39yJ5pGDTMfYRJ").unwrap(),
        Pubkey::from_str("5PjJbJsJvpCfmunbX87XQ7HyPM3pZnqaKvna3aMhYj3g").unwrap(),
        Pubkey::from_str("5PjM4nvbSbAUR93VUBWDaawBfXL3Vs1A6Anj4cqUGkgN").unwrap(),
        Pubkey::from_str("5PjG8yVnyVhJaEYMJbDZ2xqZseooWET28RYnj8aaWLSM").unwrap(),
        Pubkey::from_str("5PjGwohcmKQMsfGBhyA8dVNkZuCSNVNtvY1ZWT2RfVSq").unwrap(),
        Pubkey::from_str("5PjKhd57Y3edCPjAi8g2tYin6v8rWg5tgtbFLoMdXJC3").unwrap(),
        Pubkey::from_str("5PjNGUJCf43TyLu8epnkTPGPaPgDYZwxnqU9Nkxsrvxu").unwrap(),
        Pubkey::from_str("5PjHzCrAFMQqw63VuLdNs15bR7gCRuqfGLXY5fbcsgmV").unwrap(),
        Pubkey::from_str("5PjK1pT77m3guMobMu47SvghVBSFXArkcpNvddw1qz1H").unwrap(),
        Pubkey::from_str("5PjKYbyGq9ZiKnqDEhL5F5nsNGEU31Hw18jnw9eETSf1").unwrap(),
        Pubkey::from_str("5PjKeH6BXPJ641aEcmzVfauHskdhWhgKoig1axt8ngn5").unwrap(),
        Pubkey::from_str("5PjLSzFhr1fRTcf63rXNijseRr81qKGw2SEPMub5GPTm").unwrap(),
        Pubkey::from_str("5PjGdwD4dWJKr8VXcdNLh55ZtGufi8XzUNWY2uxcSuig").unwrap(),
        Pubkey::from_str("5PjJAFog5MJrMVLSad4AV8zZwDC1gN5ZZFQtoJ65U5Xn").unwrap(),
        Pubkey::from_str("5PjJfXZagepk31V1u5tAYnCfqsrbVFsa7b17PPLAnh5h").unwrap(),
        Pubkey::from_str("FUBad9ZBZmegSugRt7qY4uM1yRxzhBRpywwkn4mRQAeQ").unwrap(),
        Pubkey::from_str("5PjPkMY8w4vmAwhr8Hi6uspKZF1EXi2fLRYHzeJEhzmU").unwrap(),
        Pubkey::from_str("7bqCZ88nK4nU3trSFYZ5TAoTVQ3GLsBuKE8CBxt2nUe2").unwrap(),
        Pubkey::from_str("5PjJtkoKw36kmQ8qG2kdtyGQ7arNtzwGgT39rSUpeHHp").unwrap(),
        Pubkey::from_str("5PjPUhrWncCfGWMktfhxVC1K6UkSupGXrNttakB8z46o").unwrap(),
        Pubkey::from_str("5PjJvu1ZLYtAybWqKgdHd6LicbGexfyY4y4Ejb4ok2A9").unwrap(),
        Pubkey::from_str("5PjP7eqDAnevbeGKr8bKGuvRGEmgUE1q9p2MKiC8c5qz").unwrap(),
        Pubkey::from_str("5PjJ3gRx5bUjPcpagrcM3cwJi3XqQ8MwFtrZFGKVJPjv").unwrap(),
        Pubkey::from_str("5PjPUVw6tR24rupUdYsQcu7oQENTbRZyjXeKSFTUfJZY").unwrap(),
        Pubkey::from_str("5PjKKWcJs6w2J5SQqG8FeKUJ37e3X3A3nxRAfdgb8PL3").unwrap(),
        Pubkey::from_str("5PjKKXX3EjxxjpcGMMe4vru4kahetzesjBV7wzM5TP6v").unwrap(),
        Pubkey::from_str("5PjJLNS3DVgpipCrxCwJY7rWpj1gHMJeywjxVJNyTZnV").unwrap(),
        Pubkey::from_str("5PjGV9A8sDeTUwEu58amnFKnUWqWjh3ctr35C7maTVky").unwrap(),
        Pubkey::from_str("5PjJyoP4cxSkxLvvtqxnNgxYBgfFRxD3Cgb2ZQea3MPR").unwrap(),
        Pubkey::from_str("5PjKr6DD9K5dJeTWnDRjkA1ShwQ7YpcHbiMLTkEhorVu").unwrap(),
        Pubkey::from_str("5PjNUtxa8PKbsBuVkrsXWExkN3BA1pCLruP599zbKM88").unwrap(),
        Pubkey::from_str("5PjM6X633xjqX3cReKeD9drDRo9nazM3LEwckCoJhkQC").unwrap(),
        Pubkey::from_str("9yM42HMJnN69rhMGr8nCYSRtFxjWTWm5Z6GeucyLBEHg").unwrap(),
        Pubkey::from_str("5PjGQ3F1Gs9vWX43CfD1ZSvaK6rqkFSxDoSjBMugbrR3").unwrap(),
        Pubkey::from_str("5PjG7qUPLrdv3cVEdEZqpYVqyBa2myYzHsifSZycmhRN").unwrap(),
        Pubkey::from_str("5PjNLuuWcopU6kTGQHL15fg1iTzPCYvxpRn9yQLT9MnP").unwrap(),
        Pubkey::from_str("5PjGAzgraAKJhYkkfGnemMr6wrTsb7v5Q4JhNQMzumuW").unwrap(),
        Pubkey::from_str("5PjLLr3TjFn4VwLGuXGNWPVPiS7EJahZguStBSeBFFrs").unwrap(),
        Pubkey::from_str("5PjHckeWWQTLv6CS8CuXbwvKqDEPjfycwCuDQXeZ2yof").unwrap(),
        Pubkey::from_str("5PjPb7HAMdgES8yJ2G4Po5hQPWqnkTKeS917i46s2z9f").unwrap(),
        Pubkey::from_str("5PjPeWhtoZ8ghg4AtKJ7mZvt3g8DrSqKWm7dJaEGvJTx").unwrap(),
        Pubkey::from_str("5PjMgWxuha8zMxMN32i57AnMC7BALLcMWiAAXpEvzjkX").unwrap(),
        Pubkey::from_str("5PjGqEtEDDq3CKbjhv9DDNfhcdBLpfQi98Q1UubZcp46").unwrap(),
        Pubkey::from_str("5PjLknVAYrx9GNtCyKQc64odFnprSdCkCjv5NyJbUoc5").unwrap(),
        Pubkey::from_str("5PjJ6fsrFC4GuMmm7sG3e1gcTLB3qRr7aJZASm3ftRJe").unwrap(),
        Pubkey::from_str("5PjJXwjsnvbfN7eQLxZDV2aVa28xfgr6tmvYnwiCMz8n").unwrap(),
        Pubkey::from_str("5PjJVYiTgGapv4SGSh72taT4QdHsZf8NwcEeUBxpQ6ys").unwrap(),
        Pubkey::from_str("5PjKbR9qMgTtQt5pe3ACBRVnQbuUAyx4M4NNGUkGqMLR").unwrap(),
        Pubkey::from_str("5PjMinT32HUbQrSGmbTHCSw1UyE7QdPtDZYar1HizVjj").unwrap(),
        Pubkey::from_str("5PjPRuYDVcWDXRUBsvEXJgBDP2Gvzte8SQkWiKoJtNry").unwrap(),
        Pubkey::from_str("5PjMvFQFSGZN4YmXBUsz48h97muAXqFuegqp22SuhU5j").unwrap(),
        Pubkey::from_str("5PjJAY6Ro4LDCpcF6zRTeU9HPoS1vN27PYJFCcB32Z9a").unwrap(),
        Pubkey::from_str("5PjJo897WTDaretivD2wPDjnq6zmtAK5h7fypPVU2Ucy").unwrap(),
        Pubkey::from_str("5PjN6aLNhK3UoX7d4w4GNp5AUNr7sf6rgrhezeevXp9B").unwrap(),
        Pubkey::from_str("5PjHJhAv31qiengHWHkbg7z8ks6zmMRW72dC7K5EgABZ").unwrap(),
        Pubkey::from_str("5PjGgxhdPmAj9ACji6Lfe7y7eZkGEEotU17CHUa1rxpA").unwrap(),
        Pubkey::from_str("5PjPaRCdSJP36A2MXNWJGeC5vTLspZQncSUSLJj4R87Y").unwrap(),
        Pubkey::from_str("5PjMC3x5PdCZmrS6mFE2VcxAr6zUnMUrKWQziJCproNX").unwrap(),
        Pubkey::from_str("5PjJ4kZkt3kpptpttRem9jhmXMaxEw478SMNNLCg6f6j").unwrap(),
        Pubkey::from_str("5PjJi5Lh5DGH1AGtS62e9BT6659Wb4MCWiNe9PEqLwvH").unwrap(),
        Pubkey::from_str("5PjNcMnJY3AdoRRUuqm2oA9HbFoxAfsCYisQMPdNYowE").unwrap(),
        Pubkey::from_str("5PjGnJRFou4UjQ2RZMeUaZwwAXZxU2Vs2wYiyre2h8Jh").unwrap(),
        Pubkey::from_str("5PjL6jcpbK2oen75KmS7HN1LAiHJ3uympNAv8ApBTyrH").unwrap(),
        Pubkey::from_str("5PjMifDGg83fkK3Lbup5qmc9zHmRPfFJQLj7zN1iUC6v").unwrap(),
        Pubkey::from_str("5PjNax3UJtuNp1AMhEWjKAXmueQNEmu8gW9Nbd4uqaw8").unwrap(),
        Pubkey::from_str("5PjNP95nMpF1CuzK6yDWkPVSGkAAsJRju258a8HBSLNv").unwrap(),
        Pubkey::from_str("5PjPUoct9U9DnEJ38zs6tesYXVHjvDPmE3nobBAC1LEm").unwrap(),
        Pubkey::from_str("5PjMPJ4GJHcB3oioAxb8n3vH8B9H7k7hSKxNnXCTManh").unwrap(),
        Pubkey::from_str("5PjHytKV8RCugb6Vs9YvK6KHbWRDTYyKxDAujFBNXhBt").unwrap(),
        Pubkey::from_str("5PjNaFFtxE21CLqQ7ezHmzBtQ2ek7NFksAF2fUvTE8yo").unwrap(),
        Pubkey::from_str("5PjJUPp6AfuEnALgUGoCTGRJELNnJcbsgxSSr2X5hjTD").unwrap(),
        Pubkey::from_str("5PjKeKSuFRbiV7ageku3sdKYnLydx2GpLEUqakscKeWr").unwrap(),
        Pubkey::from_str("5PjNY5j7JY1WcPonEBUB8EbFsgs9fdrw62gcVTW61prJ").unwrap(),
        Pubkey::from_str("5PjHJfBDCXGNL6F3rNvQHUghhCLQANX4m8AGZN1TKUrj").unwrap(),
        Pubkey::from_str("5PjMAirq9nY7Vm4jn1H7T3ffCBXjv4FmQpKBP8krk6EX").unwrap(),
        Pubkey::from_str("5PjMVXLKue2GDSDfPzCSgPaXNuVnruTrbewC9SQRMYUz").unwrap(),
        Pubkey::from_str("5PjHt9ey1QTYfqcFG2H2q1VRu7tToRYuJ3QD7ssi2jro").unwrap(),
        Pubkey::from_str("5PjGSTuhvB1hzqojd9r4CwErMYNG1ERbwf35reQaS2FN").unwrap(),
        Pubkey::from_str("5PjH7gy16DwaKecR4iTi7KcgMTvWy45sxPsYEkkk2tTj").unwrap(),
        Pubkey::from_str("5PjGFAzSophyMAZXogGMTdvGTceD3aojyuma9D7ZpAdv").unwrap(),
        Pubkey::from_str("5PjNhSTVVWDwcGZzRq2byqhv7nZ9ZUx2vvMwMmBRiRTG").unwrap(),
        Pubkey::from_str("5PjHr7bc3SxT2NZpYTk9VZmWwKv7r3tvEHJDDUJvmKgV").unwrap(),
        Pubkey::from_str("5PjMKye8bnJSo8vphMJaVrydbKMSrCoK3uk5wnyt8fhW").unwrap(),
        Pubkey::from_str("5PjJSnRJMUxpuPQFrq8oJMDfwaCXTLSK8eitkMGBevkJ").unwrap(),
        Pubkey::from_str("5PjNfcC5VbSAcHDJa18WUK7aJ612EqM2hEEWtnp1dGEB").unwrap(),
        Pubkey::from_str("5fhDMuGKRDPWVWXf7BBEwifRFrp6XwXctDQoG7UHGVt6").unwrap(),
        Pubkey::from_str("5mrUN7rG14e88H2vccp3z1zHvc37DDMhD8DELMPftSp9").unwrap(),
        Pubkey::from_str("5PjHxBs6aXUEoGQmEUjrjtAVcRG9t3wEs8TYvpj1avAP").unwrap(),
        Pubkey::from_str("6FPiprpfDmnLkmrQiiHrGWeHTf8tRBUjKT3jzX3DPbP2").unwrap(),
        Pubkey::from_str("5PjMdKD7QSgqnUorMZWKKybob8A17gcMAoTNhRyLn32c").unwrap(),
        Pubkey::from_str("5PjM1ZCLoFJy2bcqoaaDNAevrpWNtT7A4N2SYJcLNGFL").unwrap(),
        Pubkey::from_str("Bpva88AdDL3Ct3YZyhqigLumMPcJozfRPzBbQkGry7AA").unwrap(),
        Pubkey::from_str("5PjJyME9xxgqSc5Zk8ZDEvhrArMod4ceaYRcLukzcZvA").unwrap(),
        Pubkey::from_str("5PjKNFm3rBhszknmKVtcY2tT6ZaJFi5EHhvwNsJsYGMg").unwrap(),
        Pubkey::from_str("5PjLHsZHk3d8QPPnMoGj5iRgnj56j6qszWSifsC8n9Y4").unwrap(),
        Pubkey::from_str("5PjLLFRhydNZy7P2RSJqYb2C6HVZFzcbdviouiJuAotv").unwrap(),
        Pubkey::from_str("5PjKaxCfAFRG9tqpWHLoH4fnqUQqim8tjzvELoUdy6Ek").unwrap(),
        Pubkey::from_str("Ayddz1UuQMKcYWhNTYAVaE2BATWNqD9yruMT83mk9ezs").unwrap(),
        Pubkey::from_str("5PjHb49LavX7FLXXw4Ha9KUikHSgvwtcf3pX4uRE8QjY").unwrap(),
        Pubkey::from_str("5PjJnE2ancYASoZUkFzBEr6v4WFMUoqaAF5Uyerpc7bG").unwrap(),
        Pubkey::from_str("5PjKsahDYuamcEsQnBxo8FEnf8GtsaQHgL8UiYZwpNzp").unwrap(),
        Pubkey::from_str("5PjGJFUL8vuzVC2VNes6zkGRnwu166tAdrkZ7wYdNhY4").unwrap(),
        Pubkey::from_str("5PjNR1CdvPmD7yGjVdrfEVtsDt1XqGq3DMXdFCsYLUTj").unwrap(),
        Pubkey::from_str("5PjKJFaJoFc8s7VkCmPFgpS5N9rAW5PLzxwqTfJwCwaD").unwrap(),
        Pubkey::from_str("5PjMA27LUKoDE6rGqdkYnsBDZASRhoA6rT1bAz49RqqK").unwrap(),
        Pubkey::from_str("5PjGTaWc2omJCeMcJTJMf4akYnxeo4upQFV3pEsCdMp8").unwrap(),
        Pubkey::from_str("5PjKhba6LYThqUwvYrX47SVcQqrHdbHQiCAwpVeubv2g").unwrap(),
        Pubkey::from_str("5PjLvhxa49E4EteNiXGu2LZmauKwv36eaCX8AsTQNCrG").unwrap(),
        Pubkey::from_str("5PjJyuCBgUNc5U8CEHRLUZZx4nCYVAZPB8mvozcB6VkQ").unwrap(),
        Pubkey::from_str("5PjGE68Zwybq3FV5F4idGsszhHSbnPBTdqbY5Vd52Qxc").unwrap(),
        Pubkey::from_str("5PjG85mRBau6CY5vQPSPKc8t3ugY11yJtCy2u9Wex4VT").unwrap(),
        Pubkey::from_str("5PjNkxszqYkpaYJAQmTLLRTrMiEZaUuzdJjtMaoCFWUJ").unwrap(),
        Pubkey::from_str("5PjNzvFvERne7c9SNPUcoyxBhGs6cGLUPx8iBNs9tzkP").unwrap(),
        Pubkey::from_str("5PjHWBn7n6YxmgM2E7Tqh92uJRY9miCFuHA3fcGZ7t25").unwrap(),
        Pubkey::from_str("5PjPU26ksAQVrNGFANshh6Q19QURnuQ6aiYNzimx1jvS").unwrap(),
        Pubkey::from_str("5PjHWDkjJakdZq8xtHQ5Ty4tRuPNYt6212FMqRFDKAEK").unwrap(),
        Pubkey::from_str("5PjNuJtoZcFuViDHfrh6BbeXiEQeB3j3zTvP8xXAAkhK").unwrap(),
        Pubkey::from_str("5PjLmCzsWMoRpYcDb1mNCN1D4GAu3p7dX3SDPgadrhLC").unwrap(),
        Pubkey::from_str("5PjNrNmpTg7NRXmsNTypvm3i71VXxpTPAvYvxvMeCWfH").unwrap(),
        Pubkey::from_str("5PjKxVxhp83KhYUhG1J7rjmBFUYbiK9MxuqC1H555unn").unwrap(),
        Pubkey::from_str("5PjMfp3iPatvQbmopjzWNKaHWqvUscdzKVSwaGTxdXjm").unwrap(),
        Pubkey::from_str("5PjM3eeQ16sa9mAxzUhxr3b6sMqzpZscixoUeK5ahwNL").unwrap(),
        Pubkey::from_str("5PjLEQhLU5kXERWeckm5vn6tVRzrkMNkTMEYxJgGaDZ4").unwrap(),
        Pubkey::from_str("5PjNqgKKyyQAsSq2FCi1pu764pUUHn5oMU9wj7mg3nr8").unwrap(),
        Pubkey::from_str("5PjJfuKcjdQ7szFTuQEJDia3pJPG3g3FK1nQixj9RG22").unwrap(),
        Pubkey::from_str("5PjJn2ZsARCP1qncJTdojjyYgap3SQzjxBMAS5GqSpBG").unwrap(),
        Pubkey::from_str("5PjNwBSRpxANuAEbE1dZo2DT6u8YpN6zp2jUxrQjLoJx").unwrap(),
        Pubkey::from_str("5PjLn73H8nUQvFVYx4Zdx9YZHpeL92QWG7X7oebkuy2v").unwrap(),
        Pubkey::from_str("5PjK7KEo7MUvZesrAaNgMHEN9Go66nBx3EjSUteijjUL").unwrap(),
        Pubkey::from_str("5PjHaakHZFGfWULb9CdzpeJEkog7FXKf5VyQt1GLHCRG").unwrap(),
        Pubkey::from_str("5PjP1Ej5q1i7GRSn9eDe9MFyyfwwSDtczaRE9hYuEhMC").unwrap(),
        Pubkey::from_str("5PjGTDZYQCMHVwrKnuF9o7WFi1uQvudgRRwo1mfdeQKm").unwrap(),
        Pubkey::from_str("5PjL5jBmD35y69gkctoxxV9K7XQ9p1p4n485Ju7zZUZp").unwrap(),
        Pubkey::from_str("5PjH5zu3RJUTKKks3vVpgJeB32d8Ed5PJavZS84xJQyE").unwrap(),
        Pubkey::from_str("5PjHcVfyjzzDVqW7sb9G7mSA9NdpZJk5GkBPN64rNTkS").unwrap(),
        Pubkey::from_str("5PjHUH3GNtbhYTii8NrZ1mAkh3DB8jB9QESfMruYLbAh").unwrap(),
        Pubkey::from_str("5PjNnyYoo8PQDTxoGXUkqTevPD8qbiutUh3P7LuvT4uj").unwrap(),
        Pubkey::from_str("5PjGQN996dcixoqqsMCGJ8D4Fcd5cxfm5LqAFasDp4NV").unwrap(),
        Pubkey::from_str("6V3dURW8Hn7Vsw4teZ3mvqk9N8GotA1mMkoeHkyjYmgF").unwrap(),
        Pubkey::from_str("5PjHr9wCUoxjTcp4rPwrYEfWa7XGP8v86rtgWmyqSrWi").unwrap(),
        Pubkey::from_str("5PjLaHr1HqrzhUB2eRWyYetzLJFnobY4UTip8ReqaamW").unwrap(),
        Pubkey::from_str("CX83CaJ29cJLHVGEEnwJGSJvwtsRjeneKEsysvYiR6SA").unwrap(),
        Pubkey::from_str("5PjMF9zB3BsANzbF5CtEHqMq8ULV1FVi73AgmDCjXWzi").unwrap(),];
        let mut h = HashMap::default();
    i2.iter().enumerate().for_each(|(i, k)| {
        h.insert(*k, i);
    });
    h
};
}

/// specify a slot
/// and keep track of epoch info for that slot
/// The idea is that algorithms often iterate over a storage slot or over a max/bank slot.
/// The epoch info for that slot will be fixed for many pubkeys, so save the calculated results.
/// There are 2 slots required for rent collection algorithms. Some callers have storage slot fixed
///  while others have max/bank slot fixed. 'epoch_info' isn't always needed, so it is optionally
///  specified by caller and only used by callee if necessary.
#[derive(Default, Copy, Clone)]
pub struct SlotInfoInEpoch {
    /// the slot
    slot: Slot,
    /// possible info about this slot
    epoch_info: Option<SlotInfoInEpochInner>,
}

/// epoch info for a slot
#[derive(Default, Copy, Clone)]
pub struct SlotInfoInEpochInner {
    /// epoch of the slot
    epoch: Epoch,
    /// partition index of the slot within the epoch
    partition_index: PartitionIndex,
    /// number of slots in this epoch
    slots_in_epoch: Slot,
}

impl SlotInfoInEpoch {
    /// create, populating epoch info
    #[allow(dead_code)]
    pub fn new(slot: Slot, epoch_schedule: &EpochSchedule) -> Self {
        let mut result = Self::new_small(slot);
        result.epoch_info = Some(result.get_epoch_info(epoch_schedule));
        result
    }
    /// create, without populating epoch info
    pub fn new_small(slot: Slot) -> Self {
        SlotInfoInEpoch {
            slot,
            ..SlotInfoInEpoch::default()
        }
    }
    /// get epoch info by returning already calculated or by calculating it now
    pub fn get_epoch_info(&self, epoch_schedule: &EpochSchedule) -> SlotInfoInEpochInner {
        if let Some(inner) = &self.epoch_info {
            *inner
        } else {
            let (epoch, partition_index) = epoch_schedule.get_epoch_and_slot_index(self.slot);
            SlotInfoInEpochInner {
                epoch,
                partition_index,
                slots_in_epoch: epoch_schedule.get_slots_in_epoch(epoch),
            }
        }
    }
}

impl ExpectedRentCollection {
    /// 'account' is being loaded from 'storage_slot' in 'bank_slot'
    /// adjusts 'account.rent_epoch' if we skipped the last rewrite on this account
    pub fn maybe_update_rent_epoch_on_load(
        account: &mut AccountSharedData,
        storage_slot: &SlotInfoInEpoch,
        bank_slot: &SlotInfoInEpoch,
        epoch_schedule: &EpochSchedule,
        rent_collector: &RentCollector,
        pubkey: &Pubkey,
        rewrites_skipped_this_slot: &Rewrites,
    ) {
        let result = Self::get_corrected_rent_epoch_on_load(
            account,
            storage_slot,
            bank_slot,
            epoch_schedule,
            rent_collector,
            pubkey,
            rewrites_skipped_this_slot,
        );
        if let Some(rent_epoch) = result {
            account.set_rent_epoch(rent_epoch);
        }
    }

    /// 'account' is being loaded
    /// we may need to adjust 'account.rent_epoch' if we skipped the last rewrite on this account
    /// returns Some(rent_epoch) if an adjustment needs to be made
    /// returns None if the account is up to date
    fn get_corrected_rent_epoch_on_load(
        account: &AccountSharedData,
        storage_slot: &SlotInfoInEpoch,
        bank_slot: &SlotInfoInEpoch,
        epoch_schedule: &EpochSchedule,
        rent_collector: &RentCollector,
        pubkey: &Pubkey,
        rewrites_skipped_this_slot: &Rewrites,
    ) -> Option<Epoch> {
        if let RentResult::CollectRent((next_epoch, rent_due)) =
            rent_collector.calculate_rent_result(pubkey, account, None)
        {
            if rent_due != 0 {
                // rent is due on this account in this epoch, so we did not skip a rewrite
                return None;
            }

            // grab epoch infno for bank slot and storage slot
            let bank_info = bank_slot.get_epoch_info(epoch_schedule);
            let (current_epoch, partition_from_current_slot) =
                (bank_info.epoch, bank_info.partition_index);
            let storage_info = storage_slot.get_epoch_info(epoch_schedule);
            let (storage_epoch, storage_slot_partition) =
                (storage_info.epoch, storage_info.partition_index);
            let partition_from_pubkey =
                Bank::partition_from_pubkey(pubkey, bank_info.slots_in_epoch);
            let mut possibly_update = true;
            if current_epoch == storage_epoch {
                // storage is in same epoch as bank
                if partition_from_pubkey > partition_from_current_slot {
                    // we haven't hit the slot's rent collection slot yet, and the storage was within this slot, so do not update
                    possibly_update = false;
                }
            } else if current_epoch == storage_epoch + 1 {
                // storage is in the previous epoch
                if storage_slot_partition >= partition_from_pubkey
                    && partition_from_pubkey > partition_from_current_slot
                {
                    // we did a rewrite in last epoch and we have not yet hit the rent collection slot in THIS epoch
                    possibly_update = false;
                }
            } // if more than 1 epoch old, then we need to collect rent because we clearly skipped it.

            let rewrites_skipped_this_pubkey_this_slot = || {
                rewrites_skipped_this_slot
                    .read()
                    .unwrap()
                    .contains_key(pubkey)
            };
            let rent_epoch = account.rent_epoch();
            if possibly_update && rent_epoch == 0 && current_epoch > 1 {
                if rewrites_skipped_this_pubkey_this_slot() {
                    return Some(next_epoch);
                } else {
                    // we know we're done
                    return None;
                }
            }

            // if an account was written >= its rent collection slot within the last epoch worth of slots, then we don't want to update it here
            if possibly_update && rent_epoch < current_epoch {
                let new_rent_epoch = if partition_from_pubkey < partition_from_current_slot
                    || (partition_from_pubkey == partition_from_current_slot
                        && rewrites_skipped_this_pubkey_this_slot())
                {
                    // partition_from_pubkey < partition_from_current_slot:
                    //  we already would have done a rewrite on this account IN this epoch
                    next_epoch
                } else {
                    // should have done rewrite up to last epoch
                    // we have not passed THIS epoch's rewrite slot yet, so the correct 'rent_epoch' is previous
                    next_epoch.saturating_sub(1)
                };
                if rent_epoch != new_rent_epoch {
                    // the point of this function:
                    // 'new_rent_epoch' is the correct rent_epoch that the account would have if we had done rewrites
                    return Some(new_rent_epoch);
                }
            } else if !possibly_update {
                // This is a non-trivial lookup. Would be nice to skip this.
                assert!(!rewrites_skipped_this_pubkey_this_slot(), "did not update rent_epoch: {}, new value for rent_epoch: {}, old: {}, current epoch: {}", pubkey, rent_epoch, next_epoch, current_epoch);
            }
        }
        None
    }

    #[allow(clippy::too_many_arguments)]
    /// it is possible 0.. rewrites were skipped on this account
    /// if so, return Some(correct hash as of 'storage_slot')
    /// if 'loaded_hash' is CORRECT, return None
    pub fn maybe_rehash_skipped_rewrite(
        loaded_account: &impl ReadableAccount,
        loaded_hash: &Hash,
        pubkey: &Pubkey,
        storage_slot: Slot,
        epoch_schedule: &EpochSchedule,
        rent_collector: &RentCollector,
        stats: &HashStats,
        max_slot_in_storages_inclusive: Slot,
        find_unskipped_slot: impl Fn(Slot) -> Option<Slot>,
        filler_account_suffix: Option<&Pubkey>,
    ) -> Option<Hash> {
        use solana_measure::measure::Measure;
        let mut m = Measure::start("rehash_calc_us");
        let expected = ExpectedRentCollection::new(
            pubkey,
            loaded_account,
            storage_slot,
            epoch_schedule,
            rent_collector,
            max_slot_in_storages_inclusive,
            find_unskipped_slot,
            filler_account_suffix,
        );

        m.stop();
        stats.rehash_calc_us.fetch_add(m.as_us(), Ordering::Relaxed);
        let expected = match expected {
            None => {
                // use the previously calculated hash
                return None;
            }
            Some(expected) => expected,
        };
        let mut m = Measure::start("rehash_hash_us");
        let recalc_hash = AccountsDb::hash_account_with_rent_epoch(
            expected.expected_rent_collection_slot_max_epoch,
            loaded_account,
            pubkey,
            expected.rent_epoch,
        );
        m.stop();
        stats.rehash_hash_us.fetch_add(m.as_us(), Ordering::Relaxed);
        if &recalc_hash == loaded_hash {
            // unnecessary calculation occurred
            stats.rehash_unnecessary.fetch_add(1, Ordering::Relaxed);
            return None;
        }
        use {log::*, std::str::FromStr};

        if let Some(i) = interesting.get(pubkey) {
            /*
            200 failed
            212
            225 failed
            226
            228 failed
            230
            231 succeeded
            234
            237 succeeded
            243 
            250 succeeded
            300 succeeded
            */
            if i < &230 {
                return None;
            }
            error!("jwash: rehashed: {} {}, slot: {}, rent_epoch: {}, existing_hash: {}, storage_slot: {}, i: {}", pubkey, recalc_hash, expected.expected_rent_collection_slot_max_epoch, expected.rent_epoch, loaded_hash, storage_slot, i);
        }
        stats.rehash_required.fetch_add(1, Ordering::Relaxed);

        // recomputed based on rent collection/rewrite slot
        // Rent would have been collected AT 'expected_rent_collection_slot', so hash according to that slot.
        // Note that a later storage (and slot) may contain this same pubkey. In that case, that newer hash will make this one irrelevant.
        Some(recalc_hash)
    }

    /// figure out whether the account stored at 'storage_slot' would have normally been rewritten at a slot that has already occurred: after 'storage_slot' but <= 'max_slot_in_storages_inclusive'
    /// returns Some(...) if the account would have normally been rewritten
    /// returns None if the account was updated wrt rent already or if it is known that there must exist a future rewrite of this account (for example, non-zero rent is due)
    fn new(
        pubkey: &Pubkey,
        loaded_account: &impl ReadableAccount,
        storage_slot: Slot,
        epoch_schedule: &EpochSchedule,
        rent_collector: &RentCollector,
        max_slot_in_storages_inclusive: Slot,
        find_unskipped_slot: impl Fn(Slot) -> Option<Slot>,
        filler_account_suffix: Option<&Pubkey>,
    ) -> Option<Self> {
        let slots_per_epoch = epoch_schedule.get_slots_in_epoch(rent_collector.epoch);

        let partition_from_pubkey =
            crate::bank::Bank::partition_from_pubkey(pubkey, slots_per_epoch);
        let (epoch_of_max_storage_slot, partition_index_from_max_slot) =
            epoch_schedule.get_epoch_and_slot_index(max_slot_in_storages_inclusive);

        // now, we have to find the root that is >= the slot where this pubkey's rent would have been collected
        let first_slot_in_max_epoch =
            max_slot_in_storages_inclusive - partition_index_from_max_slot;
        let mut expected_rent_collection_slot_max_epoch =
            first_slot_in_max_epoch + partition_from_pubkey;
        let calculated_from_index_expected_rent_collection_slot_max_epoch =
            expected_rent_collection_slot_max_epoch;
        if expected_rent_collection_slot_max_epoch <= max_slot_in_storages_inclusive {
            // may need to find a valid root
            if let Some(find) =
                find_unskipped_slot(calculated_from_index_expected_rent_collection_slot_max_epoch)
            {
                // found a root that is >= expected_rent_collection_slot.
                expected_rent_collection_slot_max_epoch = find;
            }
        }
        if expected_rent_collection_slot_max_epoch > max_slot_in_storages_inclusive {
            // max slot has not hit the slot in the max epoch where we would have collected rent yet, so the most recent rent-collected rewrite slot for this pubkey would be in the previous epoch
            expected_rent_collection_slot_max_epoch =
                calculated_from_index_expected_rent_collection_slot_max_epoch
                    .saturating_sub(slots_per_epoch);
            // since we are looking a different root, we have to call this again
            if let Some(find) = find_unskipped_slot(expected_rent_collection_slot_max_epoch) {
                // found a root (because we have a storage) that is >= expected_rent_collection_slot.
                expected_rent_collection_slot_max_epoch = find;
            }
        }

        // the slot we're dealing with is where we expected the rent to be collected for this pubkey, so use what is in this slot
        // however, there are cases, such as adjusting the clock, where we store the account IN the same slot, but we do so BEFORE we collect rent. We later store the account AGAIN for rewrite/rent collection.
        // So, if storage_slot == expected_rent_collection_slot..., then we MAY have collected rent or may not have. So, it has to be >
        // rent_epoch=0 is a special case
        if storage_slot > expected_rent_collection_slot_max_epoch
            || loaded_account.rent_epoch() == 0
        {
            // no need to update hash
            return None;
        }

        // ask the rent collector what rent should be collected.
        // Rent collector knows the current epoch.
        let rent_result =
            rent_collector.calculate_rent_result(pubkey, loaded_account, filler_account_suffix);
        let current_rent_epoch = loaded_account.rent_epoch();
        let new_rent_epoch = match rent_result {
            RentResult::CollectRent((mut next_epoch, rent_due)) => {
                if next_epoch > current_rent_epoch && rent_due != 0 {
                    // this is an account that would have had rent collected since this storage slot, so just use the hash we have since there must be a newer version of this account already in a newer slot
                    // It would be a waste of time to recalcluate a hash.
                    return None;
                }
                if first_slot_in_max_epoch > expected_rent_collection_slot_max_epoch {
                    // this account won't have had rent collected for the current epoch yet (rent_collector has a current epoch), so our expected next_epoch is for the previous epoch
                    next_epoch = next_epoch.saturating_sub(1);
                }
                std::cmp::max(next_epoch, current_rent_epoch)
            }
            RentResult::LeaveAloneNoRent => {
                // rent_epoch is not updated for this condition
                // But, a rewrite WOULD HAVE occured at the expected slot.
                // So, fall through with same rent_epoch, but we will have already calculated 'expected_rent_collection_slot_max_epoch'
                current_rent_epoch
            }
        };

        if expected_rent_collection_slot_max_epoch == storage_slot
            && new_rent_epoch == loaded_account.rent_epoch()
        {
            // no rewrite would have occurred
            return None;
        }

        Some(Self {
            partition_from_pubkey,
            epoch_of_max_storage_slot,
            partition_index_from_max_slot,
            first_slot_in_max_epoch,
            expected_rent_collection_slot_max_epoch,
            rent_epoch: new_rent_epoch,
        })
    }
}

#[cfg(test)]
pub mod tests {
    use {
        super::*,
        solana_sdk::{
            account::{AccountSharedData, WritableAccount},
            genesis_config::GenesisConfig,
        },
    };

    #[test]
    fn test_expected_rent_collection() {
        solana_logger::setup();
        let pubkey = Pubkey::new(&[5; 32]);
        let owner = solana_sdk::pubkey::new_rand();
        let mut account = AccountSharedData::new(1, 0, &owner);
        let max_slot_in_storages_inclusive = 0;
        let epoch_schedule = EpochSchedule::default();
        let first_normal_slot = epoch_schedule.first_normal_slot;
        let storage_slot = first_normal_slot;
        let epoch = epoch_schedule.get_epoch(storage_slot);
        assert_eq!(
            (epoch, 0),
            epoch_schedule.get_epoch_and_slot_index(storage_slot)
        );
        let genesis_config = GenesisConfig::default();
        let mut rent_collector = RentCollector::new(
            epoch,
            &epoch_schedule,
            genesis_config.slots_per_year(),
            &genesis_config.rent,
        );
        rent_collector.rent.lamports_per_byte_year = 0; // temporarily disable rent
        let find_unskipped_slot = Some;
        // slot in current epoch
        let expected = ExpectedRentCollection::new(
            &pubkey,
            &account,
            storage_slot,
            &epoch_schedule,
            &rent_collector,
            max_slot_in_storages_inclusive,
            find_unskipped_slot,
            None,
        );
        assert!(expected.is_none());

        let slots_per_epoch = 432_000;
        assert_eq!(
            slots_per_epoch,
            epoch_schedule.get_slots_in_epoch(epoch_schedule.get_epoch(storage_slot))
        );
        let partition_index_max_inclusive = slots_per_epoch - 1;
        account.set_rent_epoch(rent_collector.epoch);
        // several epochs ahead of now
        // first slot of new epoch is max slot EXclusive
        // so last slot of prior epoch is max slot INclusive
        let max_slot_in_storages_inclusive = slots_per_epoch * 3 + first_normal_slot - 1;
        rent_collector.epoch = epoch_schedule.get_epoch(max_slot_in_storages_inclusive);
        let partition_from_pubkey = 8470; // function of 432k slots and 'pubkey' above
        let first_slot_in_max_epoch = 1388256;
        let expected_rent_collection_slot_max_epoch =
            first_slot_in_max_epoch + partition_from_pubkey;
        let expected = ExpectedRentCollection::new(
            &pubkey,
            &account,
            storage_slot,
            &epoch_schedule,
            &rent_collector,
            max_slot_in_storages_inclusive,
            find_unskipped_slot,
            None,
        );
        assert_eq!(
            expected,
            Some(ExpectedRentCollection {
                partition_from_pubkey,
                epoch_of_max_storage_slot: rent_collector.epoch,
                partition_index_from_max_slot: partition_index_max_inclusive,
                first_slot_in_max_epoch,
                expected_rent_collection_slot_max_epoch,
                rent_epoch: rent_collector.epoch,
            })
        );

        // LeaveAloneNoRent
        for leave_alone in [true, false] {
            account.set_executable(leave_alone);
            let expected = ExpectedRentCollection::new(
                &pubkey,
                &account,
                expected_rent_collection_slot_max_epoch,
                &epoch_schedule,
                &rent_collector,
                max_slot_in_storages_inclusive,
                find_unskipped_slot,
                None,
            );
            assert_eq!(
                expected,
                (!leave_alone).then(|| ExpectedRentCollection {
                    partition_from_pubkey,
                    epoch_of_max_storage_slot: rent_collector.epoch,
                    partition_index_from_max_slot: partition_index_max_inclusive,
                    first_slot_in_max_epoch,
                    expected_rent_collection_slot_max_epoch,
                    rent_epoch: rent_collector.epoch,
                }),
                "leave_alone: {}",
                leave_alone
            );
        }

        // storage_slot > expected_rent_collection_slot_max_epoch
        // if greater, we return None
        for greater in [false, true] {
            let expected = ExpectedRentCollection::new(
                &pubkey,
                &account,
                expected_rent_collection_slot_max_epoch + if greater { 1 } else { 0 },
                &epoch_schedule,
                &rent_collector,
                max_slot_in_storages_inclusive,
                find_unskipped_slot,
                None,
            );
            assert_eq!(
                expected,
                (!greater).then(|| ExpectedRentCollection {
                    partition_from_pubkey,
                    epoch_of_max_storage_slot: rent_collector.epoch,
                    partition_index_from_max_slot: partition_index_max_inclusive,
                    first_slot_in_max_epoch,
                    expected_rent_collection_slot_max_epoch,
                    rent_epoch: rent_collector.epoch,
                })
            );
        }

        // test rewrite would have occurred in previous epoch from max_slot_in_storages_inclusive's epoch
        // the change is in 'rent_epoch' returned in 'expected'
        for previous_epoch in [false, true] {
            let expected = ExpectedRentCollection::new(
                &pubkey,
                &account,
                expected_rent_collection_slot_max_epoch,
                &epoch_schedule,
                &rent_collector,
                max_slot_in_storages_inclusive + if previous_epoch { slots_per_epoch } else { 0 },
                find_unskipped_slot,
                None,
            );
            let epoch_delta = if previous_epoch { 1 } else { 0 };
            let slot_delta = epoch_delta * slots_per_epoch;
            assert_eq!(
                expected,
                Some(ExpectedRentCollection {
                    partition_from_pubkey,
                    epoch_of_max_storage_slot: rent_collector.epoch + epoch_delta,
                    partition_index_from_max_slot: partition_index_max_inclusive,
                    first_slot_in_max_epoch: first_slot_in_max_epoch + slot_delta,
                    expected_rent_collection_slot_max_epoch: expected_rent_collection_slot_max_epoch
                        + slot_delta,
                    rent_epoch: rent_collector.epoch,
                }),
                "previous_epoch: {}",
                previous_epoch,
            );
        }

        // if account's rent_epoch is already > our rent epoch, rent was collected already
        // if greater, we return None
        let original_rent_epoch = account.rent_epoch();
        for already_collected in [true, false] {
            // to consider: maybe if we already collected rent_epoch IN this slot and slot matches what we need, then we should return None here
            account.set_rent_epoch(original_rent_epoch + if already_collected { 1 } else { 0 });
            let expected = ExpectedRentCollection::new(
                &pubkey,
                &account,
                expected_rent_collection_slot_max_epoch,
                &epoch_schedule,
                &rent_collector,
                max_slot_in_storages_inclusive,
                find_unskipped_slot,
                None,
            );
            assert_eq!(
                expected,
                Some(ExpectedRentCollection {
                    partition_from_pubkey,
                    epoch_of_max_storage_slot: rent_collector.epoch,
                    partition_index_from_max_slot: partition_index_max_inclusive,
                    first_slot_in_max_epoch,
                    expected_rent_collection_slot_max_epoch,
                    rent_epoch: std::cmp::max(rent_collector.epoch, account.rent_epoch()),
                }),
                "rent_collector.epoch: {}, already_collected: {}",
                rent_collector.epoch,
                already_collected
            );
        }
        account.set_rent_epoch(original_rent_epoch);

        let storage_slot = max_slot_in_storages_inclusive - slots_per_epoch;
        // check partition from pubkey code
        for end_partition_index in [0, 1, 2, 100, slots_per_epoch - 2, slots_per_epoch - 1] {
            // generate a pubkey range
            let range = crate::bank::Bank::pubkey_range_from_partition((
                // start_index:
                end_partition_index.saturating_sub(1), // this can end up at start=0, end=0 (this is a desired test case)
                // end_index:
                end_partition_index,
                epoch_schedule.get_slots_in_epoch(rent_collector.epoch),
            ));
            // use both start and end from INclusive range separately
            for pubkey in [&range.start(), &range.end()] {
                let expected = ExpectedRentCollection::new(
                    pubkey,
                    &account,
                    storage_slot,
                    &epoch_schedule,
                    &rent_collector,
                    max_slot_in_storages_inclusive,
                    find_unskipped_slot,
                    None,
                );
                assert_eq!(
                    expected,
                    Some(ExpectedRentCollection {
                        partition_from_pubkey: end_partition_index,
                        epoch_of_max_storage_slot: rent_collector.epoch,
                        partition_index_from_max_slot: partition_index_max_inclusive,
                        first_slot_in_max_epoch,
                        expected_rent_collection_slot_max_epoch: first_slot_in_max_epoch + end_partition_index,
                        rent_epoch: rent_collector.epoch,
                    }),
                    "range: {:?}, pubkey: {:?}, end_partition_index: {}, max_slot_in_storages_inclusive: {}",
                    range,
                    pubkey,
                    end_partition_index,
                    max_slot_in_storages_inclusive,
                );
            }
        }

        // check max_slot_in_storages_inclusive related code
        // so sweep through max_slot_in_storages_inclusive values within an epoch
        let first_slot_in_max_epoch = first_normal_slot + slots_per_epoch;
        rent_collector.epoch = epoch_schedule.get_epoch(first_slot_in_max_epoch);
        // an epoch in the past so we always collect rent
        let storage_slot = first_normal_slot;
        for partition_index in [
            0,
            1,
            2,
            partition_from_pubkey - 1,
            partition_from_pubkey,
            partition_from_pubkey + 1,
            100,
            slots_per_epoch - 2,
            slots_per_epoch - 1,
        ] {
            // partition_index=0 means first slot of second normal epoch
            // second normal epoch because we want to deal with accounts stored in the first normal epoch
            // + 1 because of exclusive
            let max_slot_in_storages_inclusive = first_slot_in_max_epoch + partition_index;
            let expected = ExpectedRentCollection::new(
                &pubkey,
                &account,
                storage_slot,
                &epoch_schedule,
                &rent_collector,
                max_slot_in_storages_inclusive,
                find_unskipped_slot,
                None,
            );
            let partition_index_passed_pubkey = partition_from_pubkey <= partition_index;
            let expected_rent_epoch =
                rent_collector.epoch - if partition_index_passed_pubkey { 0 } else { 1 };
            let expected_rent_collection_slot_max_epoch = first_slot_in_max_epoch
                + partition_from_pubkey
                - if partition_index_passed_pubkey {
                    0
                } else {
                    slots_per_epoch
                };

            assert_eq!(
                expected,
                Some(ExpectedRentCollection {
                    partition_from_pubkey,
                    epoch_of_max_storage_slot: rent_collector.epoch,
                    partition_index_from_max_slot: partition_index,
                    first_slot_in_max_epoch,
                    expected_rent_collection_slot_max_epoch,
                    rent_epoch: expected_rent_epoch,
                }),
                "partition_index: {}, max_slot_in_storages_inclusive: {}, storage_slot: {}, first_normal_slot: {}",
                partition_index,
                max_slot_in_storages_inclusive,
                storage_slot,
                first_normal_slot,
            );
        }

        // test account.rent_epoch = 0
        let first_slot_in_max_epoch = 1388256;
        for account_rent_epoch in [0, epoch] {
            account.set_rent_epoch(account_rent_epoch);
            let expected = ExpectedRentCollection::new(
                &pubkey,
                &account,
                storage_slot,
                &epoch_schedule,
                &rent_collector,
                max_slot_in_storages_inclusive,
                find_unskipped_slot,
                None,
            );
            assert_eq!(
                expected,
                (account_rent_epoch != 0).then(|| ExpectedRentCollection {
                    partition_from_pubkey,
                    epoch_of_max_storage_slot: rent_collector.epoch + 1,
                    partition_index_from_max_slot: partition_index_max_inclusive,
                    first_slot_in_max_epoch,
                    expected_rent_collection_slot_max_epoch,
                    rent_epoch: rent_collector.epoch,
                })
            );
        }

        // test find_unskipped_slot
        for find_unskipped_slot in [
            |_| None,
            Some,                  // identity
            |slot| Some(slot + 1), // increment
            |_| Some(Slot::MAX),   // max
        ] {
            let test_value = 10;
            let find_result = find_unskipped_slot(test_value);
            let increment = find_result.unwrap_or_default() == test_value + 1;
            let expected = ExpectedRentCollection::new(
                &pubkey,
                &account,
                storage_slot,
                &epoch_schedule,
                &rent_collector,
                max_slot_in_storages_inclusive,
                find_unskipped_slot,
                None,
            );
            assert_eq!(
                expected,
                Some(ExpectedRentCollection {
                    partition_from_pubkey,
                    epoch_of_max_storage_slot: rent_collector.epoch + 1,
                    partition_index_from_max_slot: partition_index_max_inclusive,
                    first_slot_in_max_epoch,
                    expected_rent_collection_slot_max_epoch: if find_result.unwrap_or_default()
                        == Slot::MAX
                    {
                        Slot::MAX
                    } else if increment {
                        expected_rent_collection_slot_max_epoch + 1
                    } else {
                        expected_rent_collection_slot_max_epoch
                    },
                    rent_epoch: rent_collector.epoch,
                })
            );
        }
    }

    #[test]
    fn test_simplified_rent_collection() {
        solana_logger::setup();
        let pubkey = Pubkey::new(&[5; 32]);
        let owner = solana_sdk::pubkey::new_rand();
        let mut account = AccountSharedData::new(1, 0, &owner);
        let mut epoch_schedule = EpochSchedule {
            first_normal_epoch: 0,
            ..EpochSchedule::default()
        };
        epoch_schedule.first_normal_slot = 0;
        let first_normal_slot = epoch_schedule.first_normal_slot;
        let slots_per_epoch = 432_000;
        let partition_from_pubkey = 8470; // function of 432k slots and 'pubkey' above
                                          // start in epoch=1 because of issues at rent_epoch=1
        let storage_slot = first_normal_slot + partition_from_pubkey + slots_per_epoch;
        let epoch = epoch_schedule.get_epoch(storage_slot);
        assert_eq!(
            (epoch, partition_from_pubkey),
            epoch_schedule.get_epoch_and_slot_index(storage_slot)
        );
        let genesis_config = GenesisConfig::default();
        let mut rent_collector = RentCollector::new(
            epoch,
            &epoch_schedule,
            genesis_config.slots_per_year(),
            &genesis_config.rent,
        );
        rent_collector.rent.lamports_per_byte_year = 0; // temporarily disable rent

        assert_eq!(
            slots_per_epoch,
            epoch_schedule.get_slots_in_epoch(epoch_schedule.get_epoch(storage_slot))
        );
        account.set_rent_epoch(1); // has to be not 0

        /*
        test this:
        pubkey_partition_index: 8470
        storage_slot: 8470
        account.rent_epoch: 1 (has to be not 0)

        max_slot: 8469 + 432k * 1
        max_slot: 8470 + 432k * 1
        max_slot: 8471 + 432k * 1
        max_slot: 8472 + 432k * 1
        max_slot: 8469 + 432k * 2
        max_slot: 8470 + 432k * 2
        max_slot: 8471 + 432k * 2
        max_slot: 8472 + 432k * 2
        max_slot: 8469 + 432k * 3
        max_slot: 8470 + 432k * 3
        max_slot: 8471 + 432k * 3
        max_slot: 8472 + 432k * 3

        one run without skipping slot 8470, once WITH skipping slot 8470
        */

        for skipped_slot in [false, true] {
            let find_unskipped_slot = if skipped_slot {
                |slot| Some(slot + 1)
            } else {
                Some
            };

            // starting at epoch = 0 has issues because of rent_epoch=0 special casing
            for epoch in 1..4 {
                for partition_index_from_max_slot in
                    partition_from_pubkey - 1..=partition_from_pubkey + 2
                {
                    let max_slot_in_storages_inclusive =
                        slots_per_epoch * epoch + first_normal_slot + partition_index_from_max_slot;
                    if storage_slot > max_slot_in_storages_inclusive {
                        continue; // illegal combination
                    }
                    rent_collector.epoch = epoch_schedule.get_epoch(max_slot_in_storages_inclusive);
                    let first_slot_in_max_epoch = max_slot_in_storages_inclusive
                        - max_slot_in_storages_inclusive % slots_per_epoch;
                    let skip_offset = if skipped_slot { 1 } else { 0 };
                    let mut expected_rent_collection_slot_max_epoch =
                        first_slot_in_max_epoch + partition_from_pubkey + skip_offset;
                    let hit_this_epoch =
                        expected_rent_collection_slot_max_epoch <= max_slot_in_storages_inclusive;
                    if !hit_this_epoch {
                        expected_rent_collection_slot_max_epoch -= slots_per_epoch;
                    }

                    assert_eq!(
                        (epoch, partition_index_from_max_slot),
                        epoch_schedule.get_epoch_and_slot_index(max_slot_in_storages_inclusive)
                    );
                    assert_eq!(
                        (epoch, 0),
                        epoch_schedule.get_epoch_and_slot_index(first_slot_in_max_epoch)
                    );
                    account.set_rent_epoch(1);
                    let expected = ExpectedRentCollection::new(
                        &pubkey,
                        &account,
                        storage_slot,
                        &epoch_schedule,
                        &rent_collector,
                        max_slot_in_storages_inclusive,
                        find_unskipped_slot,
                        None,
                    );
                    let some_expected = if epoch == 1 {
                        skipped_slot && partition_index_from_max_slot > partition_from_pubkey
                    } else if epoch == 2 {
                        partition_index_from_max_slot >= partition_from_pubkey - skip_offset
                    } else {
                        true
                    };
                    assert_eq!(
                        expected,
                        some_expected.then(|| ExpectedRentCollection {
                            partition_from_pubkey,
                            epoch_of_max_storage_slot: rent_collector.epoch,
                            partition_index_from_max_slot,
                            first_slot_in_max_epoch,
                            expected_rent_collection_slot_max_epoch,
                            rent_epoch: rent_collector.epoch - if hit_this_epoch { 0 } else { 1 },
                        }),
                        "partition_index_from_max_slot: {}, epoch: {}",
                        partition_index_from_max_slot,
                        epoch,
                    );

                    // test RentResult::LeaveAloneNoRent
                    {
                        let expected = ExpectedRentCollection::new(
                            &pubkey,
                            &account,
                            storage_slot,
                            &epoch_schedule,
                            &rent_collector,
                            max_slot_in_storages_inclusive,
                            find_unskipped_slot,
                            // treat this pubkey like a filler account so we get a 'LeaveAloneNoRent' result
                            Some(&pubkey),
                        );
                        assert_eq!(
                            expected,
                            some_expected.then(|| ExpectedRentCollection {
                                partition_from_pubkey,
                                epoch_of_max_storage_slot: rent_collector.epoch,
                                partition_index_from_max_slot,
                                first_slot_in_max_epoch,
                                expected_rent_collection_slot_max_epoch,
                                // this will not be adjusted for 'LeaveAloneNoRent'
                                rent_epoch: account.rent_epoch(),
                            }),
                            "partition_index_from_max_slot: {}, epoch: {}",
                            partition_index_from_max_slot,
                            epoch,
                        );
                    }

                    // test maybe_rehash_skipped_rewrite
                    let hash = AccountsDb::hash_account(storage_slot, &account, &pubkey);
                    let maybe_rehash = ExpectedRentCollection::maybe_rehash_skipped_rewrite(
                        &account,
                        &hash,
                        &pubkey,
                        storage_slot,
                        &epoch_schedule,
                        &rent_collector,
                        &HashStats::default(),
                        max_slot_in_storages_inclusive,
                        find_unskipped_slot,
                        None,
                    );
                    assert_eq!(
                        maybe_rehash,
                        some_expected.then(|| {
                            AccountsDb::hash_account_with_rent_epoch(
                                expected
                                    .as_ref()
                                    .unwrap()
                                    .expected_rent_collection_slot_max_epoch,
                                &account,
                                &pubkey,
                                expected.as_ref().unwrap().rent_epoch,
                            )
                        })
                    );
                }
            }
        }
    }

    #[test]
    fn test_get_corrected_rent_epoch_on_load() {
        solana_logger::setup();
        let pubkey = Pubkey::new(&[5; 32]);
        let owner = solana_sdk::pubkey::new_rand();
        let mut account = AccountSharedData::new(1, 0, &owner);
        let mut epoch_schedule = EpochSchedule {
            first_normal_epoch: 0,
            ..EpochSchedule::default()
        };
        epoch_schedule.first_normal_slot = 0;
        let first_normal_slot = epoch_schedule.first_normal_slot;
        let slots_per_epoch = 432_000;
        let partition_from_pubkey = 8470; // function of 432k slots and 'pubkey' above
                                          // start in epoch=1 because of issues at rent_epoch=1
        let storage_slot = first_normal_slot + partition_from_pubkey + slots_per_epoch;
        let epoch = epoch_schedule.get_epoch(storage_slot);
        assert_eq!(
            (epoch, partition_from_pubkey),
            epoch_schedule.get_epoch_and_slot_index(storage_slot)
        );
        let genesis_config = GenesisConfig::default();
        let mut rent_collector = RentCollector::new(
            epoch,
            &epoch_schedule,
            genesis_config.slots_per_year(),
            &genesis_config.rent,
        );
        rent_collector.rent.lamports_per_byte_year = 0; // temporarily disable rent

        assert_eq!(
            slots_per_epoch,
            epoch_schedule.get_slots_in_epoch(epoch_schedule.get_epoch(storage_slot))
        );
        account.set_rent_epoch(1); // has to be not 0

        /*
        test this:
        pubkey_partition_index: 8470
        storage_slot: 8470
        account.rent_epoch: 1 (has to be not 0)

        max_slot: 8469 + 432k * 1
        max_slot: 8470 + 432k * 1
        max_slot: 8471 + 432k * 1
        max_slot: 8472 + 432k * 1
        max_slot: 8469 + 432k * 2
        max_slot: 8470 + 432k * 2
        max_slot: 8471 + 432k * 2
        max_slot: 8472 + 432k * 2
        max_slot: 8469 + 432k * 3
        max_slot: 8470 + 432k * 3
        max_slot: 8471 + 432k * 3
        max_slot: 8472 + 432k * 3

        one run without skipping slot 8470, once WITH skipping slot 8470
        */

        for new_small in [false, true] {
            for rewrite_already in [false, true] {
                // starting at epoch = 0 has issues because of rent_epoch=0 special casing
                for epoch in 1..4 {
                    for partition_index_bank_slot in
                        partition_from_pubkey - 1..=partition_from_pubkey + 2
                    {
                        let bank_slot =
                            slots_per_epoch * epoch + first_normal_slot + partition_index_bank_slot;
                        if storage_slot > bank_slot {
                            continue; // illegal combination
                        }
                        rent_collector.epoch = epoch_schedule.get_epoch(bank_slot);
                        let first_slot_in_max_epoch = bank_slot - bank_slot % slots_per_epoch;

                        assert_eq!(
                            (epoch, partition_index_bank_slot),
                            epoch_schedule.get_epoch_and_slot_index(bank_slot)
                        );
                        assert_eq!(
                            (epoch, 0),
                            epoch_schedule.get_epoch_and_slot_index(first_slot_in_max_epoch)
                        );
                        account.set_rent_epoch(1);
                        let rewrites = Rewrites::default();
                        if rewrite_already {
                            if partition_index_bank_slot != partition_from_pubkey {
                                // this is an invalid test occurrence.
                                // we wouldn't have inserted pubkey into 'rewrite_already' for this slot if the current partition index wasn't at the pubkey's partition idnex yet.
                                continue;
                            }

                            rewrites.write().unwrap().insert(pubkey, Hash::default());
                        }
                        let expected_new_rent_epoch =
                            if partition_index_bank_slot > partition_from_pubkey {
                                if epoch > account.rent_epoch() {
                                    Some(rent_collector.epoch)
                                } else {
                                    None
                                }
                            } else if partition_index_bank_slot == partition_from_pubkey
                                && rewrite_already
                            {
                                let expected_rent_epoch = rent_collector.epoch;
                                if expected_rent_epoch == account.rent_epoch() {
                                    None
                                } else {
                                    Some(expected_rent_epoch)
                                }
                            } else if partition_index_bank_slot <= partition_from_pubkey
                                && epoch > account.rent_epoch()
                            {
                                let expected_rent_epoch = rent_collector.epoch.saturating_sub(1);
                                if expected_rent_epoch == account.rent_epoch() {
                                    None
                                } else {
                                    Some(expected_rent_epoch)
                                }
                            } else {
                                None
                            };
                        let get_slot_info = |slot| {
                            if new_small {
                                SlotInfoInEpoch::new_small(slot)
                            } else {
                                SlotInfoInEpoch::new(slot, &epoch_schedule)
                            }
                        };
                        let new_rent_epoch =
                            ExpectedRentCollection::get_corrected_rent_epoch_on_load(
                                &account,
                                &get_slot_info(storage_slot),
                                &get_slot_info(bank_slot),
                                &epoch_schedule,
                                &rent_collector,
                                &pubkey,
                                &rewrites,
                            );
                        assert_eq!(new_rent_epoch, expected_new_rent_epoch);
                    }
                }
            }
        }
    }
}
