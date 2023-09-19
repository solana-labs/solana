use {
    solana_ledger::blockstore::Blockstore,
    solana_sdk::{clock::Slot, hash::Hash, pubkey::Pubkey, timing::timestamp},
    std::{
        collections::HashMap,
        net::SocketAddr,
        sync::atomic::{AtomicUsize, Ordering},
    },
};

// Number of validators to sample for the ancestor repair
// We use static to enable tests from having to spin up 21 validators
static ANCESTOR_HASH_REPAIR_SAMPLE_SIZE: AtomicUsize = AtomicUsize::new(21);

pub fn get_ancestor_hash_repair_sample_size() -> usize {
    ANCESTOR_HASH_REPAIR_SAMPLE_SIZE.load(Ordering::Relaxed)
}

pub fn set_ancestor_hash_repair_sample_size_for_tests_only(sample_size: usize) {
    ANCESTOR_HASH_REPAIR_SAMPLE_SIZE.store(sample_size, Ordering::Relaxed);
}

// Even assuming 20% of validators malicious, the chance that >= 11 of the
// ANCESTOR_HASH_REPAIR_SAMPLE_SIZE = 21 validators is malicious is roughly 1/1000.
// Assuming we send a separate sample every 5 seconds, that's once every hour.

// On the other hand with a 52-48 split of validators with one version of the block vs
// another, the chance of >= 11 of the 21 sampled being from the 52% portion is
// about 57%, so we should be able to find a correct sample in a reasonable amount of time.
pub fn get_minimum_ancestor_agreement_size() -> usize {
    (get_ancestor_hash_repair_sample_size() + 1) / 2
}
const RETRY_INTERVAL_SECONDS: usize = 5;

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum DuplicateAncestorDecision {
    InvalidSample,
    SampleNotDuplicateConfirmed,
    ContinueSearch(DuplicateSlotRepairStatus),
    EarliestMismatchFound(DuplicateSlotRepairStatus),
    EarliestPrunedMismatchFound(DuplicateSlotRepairStatus),
}

impl DuplicateAncestorDecision {
    pub fn is_retryable(&self) -> bool {
        match self {
            // If we get a bad sample from malicious validators, then retry
            DuplicateAncestorDecision::InvalidSample
            // It may be possible the validators have not yet detected duplicate confirmation
            // so retry
            | DuplicateAncestorDecision::SampleNotDuplicateConfirmed => true,

             DuplicateAncestorDecision::ContinueSearch(_status)
            | DuplicateAncestorDecision::EarliestMismatchFound(_status)
            | DuplicateAncestorDecision::EarliestPrunedMismatchFound(_status) => false,
        }
    }

    pub fn repair_status(&self) -> Option<&DuplicateSlotRepairStatus> {
        match self {
            DuplicateAncestorDecision::InvalidSample
            | DuplicateAncestorDecision::SampleNotDuplicateConfirmed => None,

            DuplicateAncestorDecision::ContinueSearch(status)
            | DuplicateAncestorDecision::EarliestMismatchFound(status)
            | DuplicateAncestorDecision::EarliestPrunedMismatchFound(status) => Some(status),
        }
    }

    pub fn repair_status_mut(&mut self) -> Option<&mut DuplicateSlotRepairStatus> {
        match self {
            DuplicateAncestorDecision::InvalidSample
            | DuplicateAncestorDecision::SampleNotDuplicateConfirmed => None,

            DuplicateAncestorDecision::ContinueSearch(status)
            | DuplicateAncestorDecision::EarliestMismatchFound(status)
            | DuplicateAncestorDecision::EarliestPrunedMismatchFound(status) => Some(status),
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DuplicateSlotRepairStatus {
    // The first ancestor slot that is mismatched.
    // A mismatched ancestor slot is one that has been replayed (frozen)
    // that has a different hash than the one agreed upon by the sampled peers.
    //
    //
    // This is the slots that needs to be dumped in order to replay the originally requested slot.
    pub correct_ancestor_to_repair: (Slot, Hash),
    pub repair_pubkey_and_addr: Option<(Pubkey, SocketAddr)>,
    pub start_ts: u64,
}

impl DuplicateSlotRepairStatus {
    fn new(correct_ancestor_to_repair: (Slot, Hash)) -> Self {
        Self {
            correct_ancestor_to_repair,
            repair_pubkey_and_addr: None,
            start_ts: timestamp(),
        }
    }
}

#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum AncestorRequestType {
    #[default]
    DeadDuplicateConfirmed,
    PopularPruned,
}

impl AncestorRequestType {
    pub fn is_pruned(&self) -> bool {
        matches!(self, Self::PopularPruned)
    }
}

pub struct AncestorDuplicateSlotToRepair {
    // Slot that `ancestor_hashes_service` found that needs to be repaired
    pub slot_to_repair: (Slot, Hash),
    // Condition that initiated this request
    pub request_type: AncestorRequestType,
}

#[derive(Debug, PartialEq, Eq)]
pub struct AncestorRequestDecision {
    // The slot that initiated this request
    pub slot: Slot,
    // Condition which initiated this request
    pub request_type: AncestorRequestType,
    // Decision
    pub decision: DuplicateAncestorDecision,
}

impl AncestorRequestDecision {
    pub fn slot_to_repair(self) -> Option<AncestorDuplicateSlotToRepair> {
        let Self {
            request_type,
            mut decision,
            ..
        } = self;
        decision
            .repair_status_mut()
            .map(|status| AncestorDuplicateSlotToRepair {
                slot_to_repair: std::mem::take(&mut status.correct_ancestor_to_repair),
                request_type,
            })
    }

    pub fn is_retryable(&self) -> bool {
        self.decision.is_retryable()
    }
}

#[derive(Default, Clone)]
pub struct AncestorRequestStatus {
    // The mismatched slot that was the subject of the AncestorHashes(requested_mismatched_slot)
    // repair request. All responses to this request should be for ancestors of this slot.
    requested_mismatched_slot: Slot,
    // Condition which initiated this request
    request_type: AncestorRequestType,
    // Timestamp at which we sent out the requests
    start_ts: u64,
    // The addresses of the validators we asked for a response, a response is only acceptable
    // from these validators. The boolean represents whether the validator
    // has responded.
    sampled_validators: HashMap<SocketAddr, bool>,
    // The number of sampled validators that have responded
    num_responses: usize,
    // Validators who have responded to our ancestor repair requests. An entry
    // Vec<(Slot, Hash)> -> usize tells us which validators have
    // responded with the same Vec<(Slot, Hash)> set of ancestors.
    //
    // TODO: Trie may be more efficient
    ancestor_request_responses: HashMap<Vec<(Slot, Hash)>, Vec<SocketAddr>>,
}

impl AncestorRequestStatus {
    pub fn new(
        sampled_validators: impl Iterator<Item = SocketAddr>,
        requested_mismatched_slot: Slot,
        request_type: AncestorRequestType,
    ) -> Self {
        AncestorRequestStatus {
            requested_mismatched_slot,
            request_type,
            start_ts: timestamp(),
            sampled_validators: sampled_validators.map(|p| (p, false)).collect(),
            ..AncestorRequestStatus::default()
        }
    }

    /// Record the response from `from_addr`. Returns Some(DuplicateAncestorDecision)
    /// if we have finalized a decision based on the responses. We can finalize a decision when
    /// one of the following conditions is met:
    /// 1) We have heard from all the validators, OR
    /// 2) >= MINIMUM_ANCESTOR_AGREEMENT_SIZE have agreed that we have the correct versions
    /// of nth ancestor, for some `n>0`, AND >= MINIMUM_ANCESTOR_AGREEMENT_SIZE have
    /// agreed we have the wrong version of the `n-1` ancestor.
    pub fn add_response(
        &mut self,
        from_addr: &SocketAddr,
        response_slot_hashes: Vec<(Slot, Hash)>,
        blockstore: &Blockstore,
    ) -> Option<DuplicateAncestorDecision> {
        if let Some(did_get_response) = self.sampled_validators.get_mut(from_addr) {
            if *did_get_response {
                // If we've already received a response from this validator, return.
                return None;
            }
            // Mark we got a response from this validator already
            *did_get_response = true;
            self.num_responses += 1;
        } else {
            // If this is not a response from one of the sampled validators, return.
            return None;
        }

        let validators_with_same_response = self
            .ancestor_request_responses
            .entry(response_slot_hashes.clone())
            .or_default();
        validators_with_same_response.push(*from_addr);

        // If we got enough of the sampled validators to respond, we are confident
        // this is the correct set of ancestors
        if validators_with_same_response.len()
            == get_minimum_ancestor_agreement_size().min(self.sampled_validators.len())
        {
            // When we reach MINIMUM_ANCESTOR_AGREEMENT_SIZE of the same responses,
            // check for mismatches.
            return Some(
                self.handle_sampled_validators_reached_agreement(blockstore, response_slot_hashes),
            );
        }

        // If everyone responded and we still haven't agreed upon a set of
        // ancestors, that means there was a lot of disagreement and we sampled
        // a bad set of validators.
        if self.num_responses
            == get_ancestor_hash_repair_sample_size().min(self.sampled_validators.len())
        {
            info!(
                "{} return invalid sample no agreement",
                self.requested_mismatched_slot
            );
            return Some(DuplicateAncestorDecision::InvalidSample);
        }

        None
    }

    pub fn request_type(&self) -> AncestorRequestType {
        self.request_type
    }

    fn handle_sampled_validators_reached_agreement(
        &mut self,
        blockstore: &Blockstore,
        agreed_response: Vec<(Slot, Hash)>,
    ) -> DuplicateAncestorDecision {
        if agreed_response.is_empty() {
            info!(
                "{} return invalid sample not duplicate confirmed",
                self.requested_mismatched_slot
            );
            return DuplicateAncestorDecision::SampleNotDuplicateConfirmed;
        }

        if agreed_response.first().unwrap().0 != self.requested_mismatched_slot {
            return DuplicateAncestorDecision::InvalidSample;
        }

        // Recall:
        // 1) *correct* validators only respond to `AncestorHashes(slot)` repair requests IFF they
        // saw the ancestors of `slot` get duplicate confirmed, AND
        // 2) *correct* validators respond with the ancestors of slot in sequential order
        // 3) `slot` should get duplicate confirmed on only one fork in the cluster
        //
        // From 1) and 3) we can conclude that it is highly likely at least one correct
        // validator reported `agreed_response` were the duplicate confirmed ancestors of
        // `self.requested_mismatched_slot`. From 2), all the `agreed_response` ancestors
        // are ordered such that the ancestor at index `i+1` is the direct descendant of the
        // ancestor at `i`.
        let mut last_ancestor = 0;
        let mut earliest_erroring_ancestor = None;
        // Iterate from smallest to largest ancestor, performing integrity checks.
        for (i, (ancestor_slot, agreed_upon_hash)) in agreed_response.iter().rev().enumerate() {
            if i != 0 && *ancestor_slot <= last_ancestor {
                info!(
                    "{} return invalid sample out of order",
                    self.requested_mismatched_slot
                );
                // Responses were not properly ordered
                return DuplicateAncestorDecision::InvalidSample;
            }
            if *ancestor_slot > self.requested_mismatched_slot {
                // We should only get ancestors of `self.requested_mismatched_slot`
                // in valid responses
                info!(
                    "{} return invalid sample big ancestor",
                    self.requested_mismatched_slot
                );
                return DuplicateAncestorDecision::InvalidSample;
            }
            let our_frozen_hash = blockstore.get_bank_hash(*ancestor_slot);
            if let Some(our_frozen_hash) = our_frozen_hash {
                match (
                    &earliest_erroring_ancestor,
                    our_frozen_hash == *agreed_upon_hash,
                ) {
                    (
                        Some((mismatch_i, DuplicateAncestorDecision::EarliestMismatchFound(_))),
                        true,
                    ) => {
                        // It's impossible have a different version of an earlier ancestor, but
                        // then also have the same version of a later ancestor.
                        let (mismatch_slot, mismatch_agreed_upon_hash) =
                            agreed_response[*mismatch_i];
                        let mismatch_our_frozen_hash = blockstore.get_bank_hash(mismatch_slot);
                        info!(
                            "When processing the ancestor sample for {}, there was a mismatch
                            for {mismatch_slot}: we had frozen hash {:?} and the cluster agreed upon
                            {mismatch_agreed_upon_hash}. However for a later ancestor {ancestor_slot}
                            we have agreement on {our_frozen_hash} as the bank hash. This should never
                            be possible, something is wrong or the cluster sample is invalid.
                            Rejecting and queuing the ancestor hashes request for retry",
                            self.requested_mismatched_slot,
                            mismatch_our_frozen_hash
                        );
                        return DuplicateAncestorDecision::InvalidSample;
                    }
                    (
                        Some((
                            mismatch_i,
                            DuplicateAncestorDecision::EarliestPrunedMismatchFound(_),
                        )),
                        true,
                    ) => {
                        // In this case an earlier ancestor was not frozen in our blockstore,
                        // however this later ancestor is matching with our version. This most
                        // likely should never happen however it could happen if we initiate an
                        // ancestor_hashes request immediately after startup from snapshot on a
                        // pruned branch, we will have a match for the snapshot bank, however we
                        // don't have any of the ancestors frozen
                        let (mismatch_slot, mismatch_agreed_upon_hash) =
                            agreed_response[*mismatch_i];
                        info!(
                            "When processing the ancestor sample for {}, an earlier ancestor {mismatch_slot}
                            was agreed upon by the cluster with hash {mismatch_agreed_upon_hash} but not
                            frozen in our blockstore. However for a later ancestor {ancestor_slot} we have
                            agreement on {our_frozen_hash} as the bank hash. This should only be possible if
                            we have just started from snapshot and immediately encountered a duplicate block on
                            a popular pruned fork, otherwise something is seriously wrong. Continuing with the
                            repair",
                            self.requested_mismatched_slot
                        );
                    }
                    (Some(decision), true) => panic!(
                        "Programmer error, {:?} should not be set in decision loop",
                        decision
                    ),
                    (Some(_), false) => { /* Already found a mismatch, descendants continue to mismatch as well */
                    }
                    (None, true) => { /* Mismatch hasn't been found yet */ }
                    (None, false) => {
                        // A mismatch has been found
                        earliest_erroring_ancestor = Some((
                            agreed_response.len() - i - 1,
                            DuplicateAncestorDecision::EarliestMismatchFound(
                                DuplicateSlotRepairStatus::default(),
                            ),
                        ));
                    }
                }
            } else if earliest_erroring_ancestor.is_none() && self.request_type.is_pruned() {
                // If the slot we are requesting for is pruned, then the slot and many of its
                // ancestors may not have a frozen hash (unlike dead slots where all the ancestors
                // will have a frozen hash). Thus the best we can do is to compare the slot numbers
                // to find the first ancestor that has the wrong parent, or the first missing
                // ancestor.
                //
                // We return the earliest such mismatch.
                if let Ok(Some(meta)) = blockstore.meta(*ancestor_slot) {
                    if i != 0 && meta.parent_slot != Some(last_ancestor) {
                        earliest_erroring_ancestor = Some((
                            agreed_response.len() - i - 1,
                            DuplicateAncestorDecision::EarliestPrunedMismatchFound(
                                DuplicateSlotRepairStatus::default(),
                            ),
                        ));
                    }
                } else {
                    earliest_erroring_ancestor = Some((
                        agreed_response.len() - i - 1,
                        DuplicateAncestorDecision::EarliestPrunedMismatchFound(
                            DuplicateSlotRepairStatus::default(),
                        ),
                    ));
                }
            } else if earliest_erroring_ancestor.is_none() {
                // If in our current ledger, `ancestor_slot` is actually on the same fork as
                // `self.requested_mismatched_slot`, then the `frozen_hash` should not be None here.
                // This is because we had to freeze `ancestor_slot` in order to replay its descendant
                // `self.requested_mismatched_slot`.
                //
                // However, it's possible that we have a version of
                // `self.requested_mismatched_slot` that is on the wrong fork with the wrong set of
                // ancestors. In this case, we could get responses about ancestors that are not
                // ancestors of our version of `self.requested_mismatched_slot`
                //
                //  ```
                //       1 - 2 - 3 - 5' - 6 (our current fork)
                //     /
                //  0
                //     \
                //       1 - 2 - 4 - 5 - 6 (cluster agreed fork)
                // ```
                //
                // In this case, if we make a AncestorsHashes(6) request for our dead slot 6, we may
                // get a response with slot `4` in it, which is a slot that doesn't have a frozen
                // hash in blockstore yet because either:
                //
                // 1) We haven't replayed that slot yet (it's on a different fork).
                // 2) We don't have that slot yet in our ledger.
                // 3) We have the correct/incorrect version of `4`, but we may have replayed
                // it on the wrong branch and it's dead.
                //
                // We ignore such ancestors in this loop.
                //
                // Note also that besides the missing slot `4`, there are also duplicates between
                // both the forks, namely `1, 2, 5` for which we have different versions of these slots
                // in our ledger. So how do we handle such cases where there are both missing and mismatched
                // ancestors?
                //
                // There are two cases:
                // 1) The first such mismatch `first_mismatch` appears somewhere BEFORE the slot `4` that is
                // missing from our blockstore.
                // 2) The first such mismatch `first_mismatch` appears AFTER the slot `4` that is
                // missing from our blockstore.
                //
                // For (1), the earlier cases in this function will cause us to detect the
                // mismatch, and stop until that slot is dumped and repaired.
                // For (2), we continue searching until we find the mismatch. There must be a
                // mismatch for us to have played the requested slot, and that mismatch will be
                // found or in case of no mismatch the last slot (requested slot) will be dumped
                // and repaired.
                //
                // In the rare case of multiple incorrect ancestry, where multiple cases of (1) and
                // (2) are present, the accompanying code in replay `dump_then_repair`, dumps
                // descendants of the earliest mismatch that also have frozen hashes. Failing that,
                // in extreme cases another round of ancestor or replay dump then repair will be
                // necessary to fix the fork.
                //
                // On example of an extreme case requiring multiple rounds of dump then repair is as follows:
                //
                // ```
                //       1 - 2 - 3 - 5' - 6' (our current fork)
                //     /
                //  0
                //     \
                //       1 - 2 - 4 - 5 - 6 (cluster agreed fork)
                // ```
                //
                // In this case suppose we have the wrong version of 5, the correct version is
                // supposed to chain to 4, and we also have the wrong version of 6, both versions
                // chain to 5 however ours is the duplicate.
                //
                // The first round of ancestor repair will detect 5' using case (2) above, and
                // replay will dump then repair it. Upon successful replay of 5, we see that 6 is
                // still dead or incorrect hash. This will require another round of ancestor or
                // replay dump then repair to fix.

                warn!(
                    "Blockstore is missing frozen hash for slot {},
                which the cluster claims is an ancestor of dead slot {}. Potentially
                our version of the dead slot chains to the wrong fork!",
                    ancestor_slot, self.requested_mismatched_slot
                );
            }
            last_ancestor = *ancestor_slot;
        }

        if let Some((earliest_erroring_ancestor_index, mut decision)) = earliest_erroring_ancestor {
            // We found the earliest mismatch `earliest_erroring_ancestor_index`.
            // We know all slots for indexes > `earliest_erroring_ancestor_index` in
            // `agreed_response` match the version we have replayed.
            if earliest_erroring_ancestor_index == agreed_response.len() - 1 {
                // If the earliest ancestor is missing or a mismatch, then we need to keep searching
                // for earlier mismatches
                let repair_status =
                    DuplicateSlotRepairStatus::new(*agreed_response.last().unwrap());
                DuplicateAncestorDecision::ContinueSearch(repair_status)
            } else {
                // We only need to dump and repair the earliest mismatching ancestor.
                let repair_status = decision.repair_status_mut().unwrap();
                repair_status.correct_ancestor_to_repair =
                    agreed_response[earliest_erroring_ancestor_index];
                decision
            }
        } else {
            // If we haven't returned by now, this implies all the ancestors matched our versions
            // of those ancestors, or are missing or incomplete. Only slot to dump and repair is
            // `self.requested_mismatched_slot`
            let repair_status = DuplicateSlotRepairStatus::new(*agreed_response.first().unwrap());
            if self.request_type.is_pruned() {
                DuplicateAncestorDecision::EarliestPrunedMismatchFound(repair_status)
            } else {
                DuplicateAncestorDecision::EarliestMismatchFound(repair_status)
            }
        }
    }

    /// Given a timestamp in milliseconds, return if we should retry with another sample batch
    /// due to timeout
    pub fn is_expired(&self) -> bool {
        timestamp() - self.start_ts > RETRY_INTERVAL_SECONDS as u64 * 1000
    }

    #[cfg(test)]
    pub fn make_expired(&mut self) {
        self.start_ts = timestamp() - RETRY_INTERVAL_SECONDS as u64 * 1000 - 1;
    }
}

#[cfg(test)]
pub mod tests {
    use {
        super::*,
        rand::{self, seq::SliceRandom, thread_rng},
        solana_ledger::get_tmp_ledger_path_auto_delete,
        std::{collections::BTreeMap, net::IpAddr},
        tempfile::TempDir,
        trees::tr,
    };

    struct TestSetup {
        sampled_addresses: Vec<SocketAddr>,
        correct_ancestors_response: Vec<(Slot, Hash)>,
        _blockstore_temp_dir: TempDir,
        blockstore: Blockstore,
        status: AncestorRequestStatus,
    }

    fn create_rand_socket_addr() -> SocketAddr {
        let bytes: [u16; 8] = rand::random();
        let ip = IpAddr::from(bytes);
        SocketAddr::new(ip, 8080)
    }

    fn setup_add_response_test_with_type(
        request_slot: Slot,
        num_ancestors_in_response: usize,
        request_type: AncestorRequestType,
    ) -> TestSetup {
        assert!(request_slot >= num_ancestors_in_response as u64);
        let sampled_addresses: Vec<SocketAddr> = std::iter::repeat_with(create_rand_socket_addr)
            .take(get_ancestor_hash_repair_sample_size())
            .collect();

        let status = AncestorRequestStatus::new(
            sampled_addresses.iter().cloned(),
            request_slot,
            request_type,
        );
        let blockstore_temp_dir = get_tmp_ledger_path_auto_delete!();
        let blockstore = Blockstore::open(blockstore_temp_dir.path()).unwrap();

        let correct_ancestors_response: Vec<(Slot, Hash)> =
            (request_slot - num_ancestors_in_response as u64..=request_slot)
                .map(|ancestor| (ancestor, Hash::new_unique()))
                .rev()
                .collect();

        TestSetup {
            sampled_addresses,
            correct_ancestors_response,
            _blockstore_temp_dir: blockstore_temp_dir,
            blockstore,
            status,
        }
    }

    fn setup_add_response_test(request_slot: Slot, num_ancestors_in_response: usize) -> TestSetup {
        setup_add_response_test_with_type(
            request_slot,
            num_ancestors_in_response,
            AncestorRequestType::DeadDuplicateConfirmed,
        )
    }

    fn setup_add_response_test_pruned(
        request_slot: Slot,
        num_ancestors_in_response: usize,
    ) -> TestSetup {
        setup_add_response_test_with_type(
            request_slot,
            num_ancestors_in_response,
            AncestorRequestType::PopularPruned,
        )
    }

    #[test]
    fn test_add_response_invalid_peer() {
        let request_slot = 100;
        let TestSetup {
            blockstore,
            mut status,
            ..
        } = setup_add_response_test(request_slot, 10);

        // Try adding a response from an invalid peer, should not be registered
        let rand_addr = create_rand_socket_addr();
        assert!(status
            .add_response(&rand_addr, vec![(99, Hash::new_unique())], &blockstore)
            .is_none());
        assert_eq!(status.num_responses, 0);
        assert!(status.ancestor_request_responses.is_empty());
    }

    #[test]
    fn test_add_multiple_responses_same_peer() {
        let request_slot = 100;
        let TestSetup {
            sampled_addresses,
            correct_ancestors_response,
            blockstore,
            mut status,
            ..
        } = setup_add_response_test(request_slot, 10);

        // Create an incorrect response
        let mut incorrect_ancestors_response = correct_ancestors_response.clone();
        incorrect_ancestors_response.pop().unwrap();

        // Add a mixture of correct and incorrect responses from the same `responder_addr`.
        let num_repeated_responses = get_ancestor_hash_repair_sample_size();
        let responder_addr = &sampled_addresses[0];
        for i in 0..num_repeated_responses {
            let response = if i % 2 == 0 {
                // This is the first response when i == 0, so it should be the only response that
                // persists. All later responses, both correct and incorrect should be ignored
                correct_ancestors_response.clone()
            } else {
                incorrect_ancestors_response.clone()
            };
            assert!(status
                .add_response(responder_addr, response, &blockstore)
                .is_none());
            assert_eq!(status.num_responses, 1);
            assert_eq!(status.ancestor_request_responses.len(), 1);
            let correct_responses = status
                .ancestor_request_responses
                .get(&correct_ancestors_response)
                .unwrap();
            assert!(correct_responses.contains(responder_addr));
            assert_eq!(correct_responses.len(), 1);
        }
    }

    /// Add `num_correct_responses` correct responses from the sampled valdiators, and
    /// then add incorrect responses from the remaining validators.
    fn run_add_multiple_correct_and_incorrect_responses(
        incorrect_responses: Vec<(Vec<(Slot, Hash)>, usize)>,
        test_setup: &mut TestSetup,
    ) -> DuplicateAncestorDecision {
        let &mut TestSetup {
            ref sampled_addresses,
            ref correct_ancestors_response,
            ref blockstore,
            ref mut status,
            ..
        } = test_setup;

        // Generate an event order of adding correct/incorrect responses
        let events: BTreeMap<usize, Vec<(Slot, Hash)>> = incorrect_responses
            .into_iter()
            .scan(
                0,
                |total_count, /*accumulated state*/
                 (
                    incorrect_response,
                    num_responses, /*number of validators returning this response*/
                )| {
                    assert!(num_responses > 0);
                    *total_count += num_responses;
                    Some((*total_count, incorrect_response))
                },
            )
            .collect();

        let total_incorrect_responses = events.iter().last().map(|(count, _)| *count).unwrap_or(0);
        assert!(total_incorrect_responses <= get_ancestor_hash_repair_sample_size());

        let mut event_order: Vec<usize> = (0..sampled_addresses.len()).collect();
        event_order.shuffle(&mut thread_rng());

        for (event, responder_addr) in event_order.iter().zip(sampled_addresses.iter()) {
            let response = events
                .range((event + 1)..)
                .next()
                .map(|(_count, response)| response)
                .unwrap_or_else(|| correct_ancestors_response)
                .clone();

            if let Some(decision) = status.add_response(responder_addr, response, blockstore) {
                // Note we may get a decision before we've heard back from all the
                // sampled validators
                return decision;
            }
        }

        // Should never get here
        panic!("Decision must be made after hearing back from all the sampled validators");
    }

    #[test]
    fn test_add_multiple_responses_invalid_sample_no_agreement() {
        let request_slot = 100;
        let mut test_setup = setup_add_response_test(request_slot, 10);

        // Create an incorrect response
        let mut incorrect_ancestors_response_0 = test_setup.correct_ancestors_response.clone();
        incorrect_ancestors_response_0.pop().unwrap();

        // Create another incorrect response
        let mut incorrect_ancestors_response_1 = incorrect_ancestors_response_0.clone();
        incorrect_ancestors_response_1.pop().unwrap();
        let desired_incorrect_responses = vec![
            (
                incorrect_ancestors_response_0,
                get_minimum_ancestor_agreement_size() - 1,
            ),
            (incorrect_ancestors_response_1, 2),
        ];

        // Ensure that no response gets >= MINIMUM_ANCESTOR_AGREEMENT_SIZE responses
        let total_invalid_responses: usize = desired_incorrect_responses
            .iter()
            .map(|(_, count)| count)
            .sum();
        assert!(
            get_ancestor_hash_repair_sample_size() - total_invalid_responses
                < get_minimum_ancestor_agreement_size()
        );

        assert_eq!(
            run_add_multiple_correct_and_incorrect_responses(
                desired_incorrect_responses,
                &mut test_setup
            ),
            DuplicateAncestorDecision::InvalidSample
        );
    }

    #[test]
    fn test_add_multiple_responses_not_duplicate_confirmed() {
        let request_slot = 100;
        let mut test_setup = setup_add_response_test(request_slot, 10);

        // Create an incorrect response that is empty
        let incorrect_ancestors_response = vec![];
        let desired_incorrect_responses = vec![(
            incorrect_ancestors_response,
            get_minimum_ancestor_agreement_size(),
        )];

        assert_eq!(
            run_add_multiple_correct_and_incorrect_responses(
                desired_incorrect_responses,
                &mut test_setup
            ),
            DuplicateAncestorDecision::SampleNotDuplicateConfirmed
        );
    }

    #[test]
    fn test_add_multiple_responses_invalid_sample_missing_requested_slot() {
        let request_slot = 100;
        let mut test_setup = setup_add_response_test(request_slot, 10);

        // Create an incorrect response that is missing `request_slot`
        let incorrect_ancestors_response = vec![(request_slot - 1, Hash::new_unique())];
        let desired_incorrect_responses = vec![(
            incorrect_ancestors_response,
            get_minimum_ancestor_agreement_size(),
        )];

        assert_eq!(
            run_add_multiple_correct_and_incorrect_responses(
                desired_incorrect_responses,
                &mut test_setup
            ),
            DuplicateAncestorDecision::InvalidSample
        );
    }

    #[test]
    fn test_add_multiple_responses_invalid_sample_responses_not_ancestors() {
        let request_slot = 100;
        let mut test_setup = setup_add_response_test(request_slot, 10);

        // Create an incorrect response. If the agreed upon response contains
        // slots >= request_slot, we still mark the responses as invalid
        let mut incorrect_ancestors_response = test_setup.correct_ancestors_response.clone();
        incorrect_ancestors_response.push((request_slot + 1, Hash::new_unique()));
        let desired_incorrect_responses = vec![(
            incorrect_ancestors_response,
            get_minimum_ancestor_agreement_size(),
        )];

        assert_eq!(
            run_add_multiple_correct_and_incorrect_responses(
                desired_incorrect_responses,
                &mut test_setup
            ),
            DuplicateAncestorDecision::InvalidSample
        );
    }

    #[test]
    fn test_add_multiple_responses_invalid_sample_responses_out_of_order() {
        let request_slot = 100;
        let mut test_setup = setup_add_response_test(request_slot, 10);

        // Create an incorrect response that is out of order
        let mut incorrect_ancestors_response = test_setup.correct_ancestors_response.clone();
        incorrect_ancestors_response.swap_remove(0);
        let desired_incorrect_responses = vec![(
            incorrect_ancestors_response,
            get_minimum_ancestor_agreement_size(),
        )];

        assert_eq!(
            run_add_multiple_correct_and_incorrect_responses(
                desired_incorrect_responses,
                &mut test_setup
            ),
            DuplicateAncestorDecision::InvalidSample
        );
    }

    #[test]
    fn test_add_multiple_responses_invalid_sample_mismatches_then_matches() {
        let request_slot = 100;
        let mut test_setup = setup_add_response_test(request_slot, 10);

        // Insert all the correct frozen ancestors
        for &(slot, correct_hash) in &test_setup.correct_ancestors_response {
            test_setup
                .blockstore
                .insert_bank_hash(slot, correct_hash, false);
        }

        // Create an incorrect response where there is a mismatched ancestor `X`, then
        // a matching ancestor `Y > X`
        let mut incorrect_ancestors_response = test_setup.correct_ancestors_response.clone();
        incorrect_ancestors_response[5].1 = Hash::new_unique();
        let desired_incorrect_responses = vec![(
            incorrect_ancestors_response,
            get_minimum_ancestor_agreement_size(),
        )];

        assert_eq!(
            run_add_multiple_correct_and_incorrect_responses(
                desired_incorrect_responses,
                &mut test_setup
            ),
            DuplicateAncestorDecision::InvalidSample
        );
    }

    #[test]
    fn test_add_multiple_responses_start_from_snapshot_missing_then_matches() {
        let request_slot = 100;
        let mut test_setup = setup_add_response_test(request_slot, 10);

        // Only insert the later half in our blockstore
        for &(slot, correct_hash) in test_setup.correct_ancestors_response.iter().take(5) {
            test_setup
                .blockstore
                .insert_bank_hash(slot, correct_hash, false);
        }

        // We don't have the earlier ancestors because we just started up, however sample should
        // not be rejected as invalid.
        let DuplicateAncestorDecision::EarliestMismatchFound(repair_status) =
            run_add_multiple_correct_and_incorrect_responses(vec![], &mut test_setup)
        else {
            panic!("Incorrect decision")
        };
        assert_eq!(
            repair_status.correct_ancestor_to_repair,
            *test_setup.correct_ancestors_response.first().unwrap()
        );
    }

    #[test]
    fn test_add_multiple_responses_start_from_snapshot_missing_then_mismatch() {
        let request_slot = 100;
        let mut test_setup = setup_add_response_test(request_slot, 10);

        // Only insert the later half in our blockstore, however make mistakes
        for &(slot, _) in test_setup.correct_ancestors_response.iter().take(5) {
            test_setup
                .blockstore
                .insert_bank_hash(slot, Hash::new_unique(), false);
        }

        // We don't have the earlier ancestors because we just started up, however sample should
        // not be rejected as invalid.
        let repair_status =
            match run_add_multiple_correct_and_incorrect_responses(vec![], &mut test_setup) {
                DuplicateAncestorDecision::EarliestMismatchFound(repair_status) => repair_status,
                x => panic!("Incorrect decision {x:?}"),
            };

        // Expect to find the first mismatch that is present
        assert_eq!(
            repair_status.correct_ancestor_to_repair,
            test_setup.correct_ancestors_response[4]
        );
    }

    #[test]
    fn test_add_multiple_responses_ancestors_all_not_frozen() {
        let request_slot = 100;
        let mut test_setup = setup_add_response_test(request_slot, 10);

        // Create an incorrect response, but the agreed upon response will be the correct
        // one.
        let mut incorrect_ancestors_response = test_setup.correct_ancestors_response.clone();
        incorrect_ancestors_response.push((request_slot, Hash::new_unique()));
        let desired_incorrect_responses = vec![(
            incorrect_ancestors_response,
            get_minimum_ancestor_agreement_size() - 1,
        )];

        // We have no entries in the blockstore, so all the ancestors will be missing
        let DuplicateAncestorDecision::EarliestMismatchFound(repair_status) =
            run_add_multiple_correct_and_incorrect_responses(
                desired_incorrect_responses,
                &mut test_setup,
            )
        else {
            panic!("Incorrect decision")
        };
        assert_eq!(
            repair_status.correct_ancestor_to_repair,
            *test_setup.correct_ancestors_response.first().unwrap()
        );
    }

    #[test]
    fn test_add_multiple_responses_ancestors_some_not_frozen() {
        let request_slot = 100;
        let mut test_setup = setup_add_response_test(request_slot, 10);

        // Set up a situation where some of our ancestors are correct,
        // but then we fork off and are missing some ancestors like so:
        //  ```
        //                 93 - 95 - 97 - 99 - 100 (our current fork, missing some slots like 98)
        //              /
        //  90 - 91 - 92 (all correct)
        //               \
        //                 93 - 94 - 95 - 96 - 97 - 98 - 99 - 100 (correct fork)
        // ```
        let rand_num: u64 = rand::random();
        let insert_even_or_odds: u64 = rand_num % 2;
        for &(slot, correct_hash) in &test_setup.correct_ancestors_response {
            if slot <= 92 {
                test_setup
                    .blockstore
                    .insert_bank_hash(slot, correct_hash, false);
            } else if slot % 2 == insert_even_or_odds {
                // Here we either skip slot 93 or 94.
                //
                // 1) If we skip slot 93, and insert mismatched slot 94 we're testing the order of
                // events `Not frozen -> Mismatched hash` which should return
                // `EarliestMismatchFound(94)`
                //
                // 2) If we insert mismatched slot 93, and skip slot 94 we're testing the order of
                // events `Mismatched hash -> Not frozen`, which should return
                // `EarliestMismatchFound(93)`
                test_setup
                    .blockstore
                    .insert_bank_hash(slot, Hash::new_unique(), false);
            }
        }

        let repair_status =
            match run_add_multiple_correct_and_incorrect_responses(vec![], &mut test_setup) {
                DuplicateAncestorDecision::EarliestMismatchFound(repair_status) => repair_status,
                x => panic!("Incorrect decision {x:?}"),
            };

        // Expect to find 93 or 94 (see comment above)
        let expected_mismatched = test_setup
            .correct_ancestors_response
            .into_iter()
            .find(|(slot, _)| *slot == if insert_even_or_odds == 0 { 94 } else { 93 })
            .unwrap();
        assert_eq!(
            repair_status.correct_ancestor_to_repair,
            expected_mismatched
        );
    }

    #[test]
    fn test_add_multiple_responses_ancestors_all_mismatched() {
        let request_slot = 100;
        let mut test_setup = setup_add_response_test(request_slot, 10);

        // Insert all the wrong hashes for the slots
        for (slot, _) in &test_setup.correct_ancestors_response {
            test_setup
                .blockstore
                .insert_bank_hash(*slot, Hash::new_unique(), false);
        }

        // All the ancestors are mismatched, so we need to continue the search
        match run_add_multiple_correct_and_incorrect_responses(vec![], &mut test_setup) {
            DuplicateAncestorDecision::ContinueSearch(repair_status) => {
                assert_eq!(
                    repair_status.correct_ancestor_to_repair,
                    *test_setup.correct_ancestors_response.last().unwrap()
                );
            }
            x => panic!("Incorrect decision {x:?}"),
        };
    }

    #[test]
    fn test_add_multiple_responses_ancestors_some_mismatched() {
        let request_slot = 100;
        let mut test_setup = setup_add_response_test(request_slot, 10);

        // Set up a situation where some of our ancestors are correct,
        // but then we fork off with different versions of the correct slots.
        //  ```
        //                 93' - 94' - 95' - 96' - 97' - 98' - 99' - 100' (our current fork)
        //              /
        //  90 - 91 - 92 (all correct)
        //               \
        //                 93 - 94 - 95 - 96 - 97 - 98 - 99 - 100 (correct fork)
        // ```

        // Insert all the wrong hashes for the slots
        for &(slot, correct_hash) in &test_setup.correct_ancestors_response {
            if slot <= 92 {
                test_setup
                    .blockstore
                    .insert_bank_hash(slot, correct_hash, false);
            } else {
                test_setup
                    .blockstore
                    .insert_bank_hash(slot, Hash::new_unique(), false);
            }
        }

        // All the ancestors are mismatched, so we need to continue the search
        match run_add_multiple_correct_and_incorrect_responses(vec![], &mut test_setup) {
            DuplicateAncestorDecision::EarliestMismatchFound(repair_status) => {
                // Expect to find the first slot after 92 in the `correct_ancestor_to_repair`.
                let expected_mismatched = test_setup
                    .correct_ancestors_response
                    .into_iter()
                    .rev()
                    .find(|(slot, _)| *slot > 92)
                    .unwrap();
                assert_eq!(
                    repair_status.correct_ancestor_to_repair,
                    expected_mismatched,
                );
            }
            x => panic!("Incorrect decision {x:?}"),
        };
    }

    #[test]
    fn test_add_multiple_responses_ancestors_all_match() {
        let request_slot = 100;
        let mut test_setup = setup_add_response_test(request_slot, 10);

        // Insert all the correct frozen ancestors
        for &(slot, correct_hash) in &test_setup.correct_ancestors_response {
            test_setup
                .blockstore
                .insert_bank_hash(slot, correct_hash, false);
        }

        // All the ancestors matched, only the requested slot should be dumped
        let DuplicateAncestorDecision::EarliestMismatchFound(repair_status) =
            run_add_multiple_correct_and_incorrect_responses(vec![], &mut test_setup)
        else {
            panic!("Incorrect decision")
        };
        assert_eq!(
            repair_status.correct_ancestor_to_repair,
            *test_setup.correct_ancestors_response.first().unwrap()
        );
    }

    #[test]
    fn test_add_multiple_responses_pruned_all_mismatch() {
        let request_slot = 100;
        let mut test_setup = setup_add_response_test_pruned(request_slot, 10);

        // We have no entries in the blockstore, so all the ancestors will be missing
        match run_add_multiple_correct_and_incorrect_responses(vec![], &mut test_setup) {
            DuplicateAncestorDecision::ContinueSearch(repair_status) => {
                assert_eq!(
                    repair_status.correct_ancestor_to_repair,
                    *test_setup.correct_ancestors_response.last().unwrap()
                );
            }
            x => panic!("Incorrect decision {x:?}"),
        };
    }

    #[test]
    fn test_add_multiple_responses_pruned_all_match() {
        let request_slot = 100;
        let mut test_setup = setup_add_response_test_pruned(request_slot, 10);

        // Insert all the correct ancestory
        let tree = test_setup
            .correct_ancestors_response
            .iter()
            .fold(tr(request_slot + 1), |tree, (slot, _)| (tr(*slot) / tree));
        test_setup
            .blockstore
            .add_tree(tree, true, true, 2, Hash::default());

        // All the ancestors matched, only the requested slot should be dumped
        let DuplicateAncestorDecision::EarliestPrunedMismatchFound(repair_status) =
            run_add_multiple_correct_and_incorrect_responses(vec![], &mut test_setup)
        else {
            panic!("Incorrect decision")
        };
        assert_eq!(
            repair_status.correct_ancestor_to_repair,
            *test_setup.correct_ancestors_response.first().unwrap()
        );
    }

    #[test]
    fn test_add_multiple_responses_pruned_some_ancestors_missing() {
        let request_slot = 100;
        let mut test_setup = setup_add_response_test_pruned(request_slot, 10);

        // Set up a situation where some of our ancestors are correct,
        // but then we fork off and are missing some ancestors like so:
        //  ```
        //                 93 - 95 - 97 - 99 - 100 (our current fork, missing some slots like 98)
        //              /
        //  90 - 91 - 92 (all correct)
        //               \
        //                 93 - 94 - 95 - 96 - 97 - 98 - 99 - 100 (correct fork)
        // ```
        let tree = test_setup
            .correct_ancestors_response
            .iter()
            .filter(|(slot, _)| *slot <= 92 || *slot % 2 == 1)
            .fold(tr(request_slot), |tree, (slot, _)| (tr(*slot) / tree));
        test_setup
            .blockstore
            .add_tree(tree, true, true, 2, Hash::default());

        let repair_status =
            match run_add_multiple_correct_and_incorrect_responses(vec![], &mut test_setup) {
                DuplicateAncestorDecision::EarliestPrunedMismatchFound(repair_status) => {
                    repair_status
                }
                x => panic!("Incorrect decision {x:?}"),
            };

        // Expect to find first slot after 93 in the `correct_ancestor_to_repair`.
        let expected_mismatched = test_setup
            .correct_ancestors_response
            .into_iter()
            .rev()
            .find(|(slot, _)| *slot > 93)
            .unwrap();
        assert_eq!(
            repair_status.correct_ancestor_to_repair,
            expected_mismatched,
        );
    }

    #[test]
    fn test_add_multiple_responses_pruned_ancestor_is_bad() {
        let request_slot = 100;
        let mut test_setup = setup_add_response_test_pruned(request_slot, 10);

        // Set up the situation we expect to see, exactly 1 duplicate has caused this branch to
        // descend from pruned.
        // ```
        // Our fork view:
        // 90 - 91 - 92
        // 10 - 11 - 93 - 94 - 95 - 96 - 97 - 98 - 99 - 100
        //
        // Correct fork:
        // 90 - 91 - 92 - 93 - 94 - 95 - 96 - 97 - 98 - 99 - 100
        // ```
        let root_fork = tr(90) / (tr(91) / tr(92));
        let pruned_fork = [10, 11, 93, 94, 95, 96, 97, 98, 99]
            .iter()
            .rev()
            .fold(tr(100), |tree, slot| (tr(*slot) / tree));

        test_setup
            .blockstore
            .add_tree(root_fork, true, true, 2, Hash::default());
        test_setup
            .blockstore
            .add_tree(pruned_fork, true, true, 2, Hash::default());

        let repair_status =
            match run_add_multiple_correct_and_incorrect_responses(vec![], &mut test_setup) {
                DuplicateAncestorDecision::EarliestPrunedMismatchFound(repair_status) => {
                    repair_status
                }
                x => panic!("Incorrect decision {x:?}"),
            };

        // Expect to find first slot after 92 in the `correct_ancestor_to_repair`.
        let expected_mismatched = test_setup
            .correct_ancestors_response
            .into_iter()
            .rev()
            .find(|(slot, _)| *slot >= 93)
            .unwrap();
        assert_eq!(
            repair_status.correct_ancestor_to_repair,
            expected_mismatched,
        );
    }
}
