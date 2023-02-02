use {super::*, crate::entry_verification_service::Results};

macro_rules! assert_non_empty_results_in_millis {
    (
        $millis:expr,
        $results:expr,
        $expected:expr $(,)?
    ) => {
        let deadline = Instant::now() + Duration::from_millis($millis);
        let results = $results.check_and_wait_until(deadline, |results| {
            if results.slots.is_empty() {
                None
            } else {
                Some(results.clone())
            }
        });
        let actual = results.expect(&format!(
            "`slots` is non-empty within {} milliseconds",
            $millis
        ));
        let expected = $expected;
        pretty_assertions_sorted::assert_eq_sorted!(
            actual,
            expected,
            "`check_and_wait_until() result does not match the expectation.\n\
             Actual: {actual:#?}\n\
             Expected: {expected:#?}"
        );
    };
}

mod advance_verified {
    use {super::*, pretty_assertions_sorted::assert_eq_sorted};

    #[test]
    fn single_slot_advance() {
        let slot = 3;
        let mut processing_slots = HashMap::from([(
            slot,
            SlotStatus::Processing {
                parent_slot: slot - 1,
                verified_up_to: 0.into(),
                last_data_set: None,
                data_sets: HashMap::from([
                    (
                        0.into(),
                        DataSetOptimisticStatus::Verified {
                            next_data_set: 7.into(),
                        },
                    ),
                    (
                        7.into(),
                        DataSetOptimisticStatus::Verified {
                            next_data_set: 11.into(),
                        },
                    ),
                    (
                        11.into(),
                        DataSetOptimisticStatus::AllButFirst {
                            first_entry: Entry::default(),
                            next_data_set: 19.into(),
                        },
                    ),
                ]),
            },
        )]);

        let updated = advance_verified(&mut processing_slots, vec![slot]);
        assert_eq!(updated, vec![slot]);
        assert_eq_sorted!(
            processing_slots,
            HashMap::from([(
                slot,
                SlotStatus::Processing {
                    parent_slot: slot - 1,
                    verified_up_to: 11.into(),
                    last_data_set: None,
                    data_sets: HashMap::from([
                        (
                            0.into(),
                            DataSetOptimisticStatus::Verified {
                                next_data_set: 7.into(),
                            },
                        ),
                        (
                            7.into(),
                            DataSetOptimisticStatus::Verified {
                                next_data_set: 11.into(),
                            },
                        ),
                        (
                            11.into(),
                            DataSetOptimisticStatus::AllButFirst {
                                first_entry: Entry::default(),
                                next_data_set: 19.into(),
                            },
                        ),
                    ]),
                },
            )]),
        );
    }

    #[test]
    fn two_slot_advance() {
        let slot1 = 3;
        let slot2 = 7;
        let mut processing_slots = HashMap::from([
            (
                slot1,
                SlotStatus::Processing {
                    parent_slot: slot1 - 1,
                    verified_up_to: 0.into(),
                    last_data_set: None,
                    data_sets: HashMap::from([
                        (
                            0.into(),
                            DataSetOptimisticStatus::Verified {
                                next_data_set: 7.into(),
                            },
                        ),
                        (
                            7.into(),
                            DataSetOptimisticStatus::Verified {
                                next_data_set: 11.into(),
                            },
                        ),
                    ]),
                },
            ),
            (
                slot2,
                SlotStatus::Processing {
                    parent_slot: slot2 - 1,
                    verified_up_to: 7.into(),
                    last_data_set: None,
                    data_sets: HashMap::from([
                        (
                            7.into(),
                            DataSetOptimisticStatus::Verified {
                                next_data_set: 13.into(),
                            },
                        ),
                        (
                            13.into(),
                            DataSetOptimisticStatus::Verified {
                                next_data_set: 17.into(),
                            },
                        ),
                    ]),
                },
            ),
        ]);

        let updated = advance_verified(&mut processing_slots, vec![slot1, slot2]);
        assert_eq!(updated, vec![slot1, slot2]);
        assert_eq_sorted!(
            processing_slots,
            HashMap::from([
                (
                    slot1,
                    SlotStatus::Processing {
                        parent_slot: slot1 - 1,
                        verified_up_to: 11.into(),
                        last_data_set: None,
                        data_sets: HashMap::from([
                            (
                                0.into(),
                                DataSetOptimisticStatus::Verified {
                                    next_data_set: 7.into(),
                                },
                            ),
                            (
                                7.into(),
                                DataSetOptimisticStatus::Verified {
                                    next_data_set: 11.into(),
                                },
                            ),
                        ]),
                    },
                ),
                (
                    slot2,
                    SlotStatus::Processing {
                        parent_slot: slot2 - 1,
                        verified_up_to: 17.into(),
                        last_data_set: None,
                        data_sets: HashMap::from([
                            (
                                7.into(),
                                DataSetOptimisticStatus::Verified {
                                    next_data_set: 13.into(),
                                },
                            ),
                            (
                                13.into(),
                                DataSetOptimisticStatus::Verified {
                                    next_data_set: 17.into(),
                                },
                            ),
                        ]),
                    },
                ),
            ]),
        );
    }

    #[test]
    fn two_slot_no_advance() {
        let slot1 = 3;
        let slot2 = 7;
        let mut processing_slots = HashMap::from([
            (
                slot1,
                SlotStatus::Processing {
                    parent_slot: slot1 - 1,
                    verified_up_to: 7.into(),
                    last_data_set: None,
                    data_sets: HashMap::from([
                        (
                            0.into(),
                            DataSetOptimisticStatus::Verified {
                                next_data_set: 7.into(),
                            },
                        ),
                        (
                            7.into(),
                            DataSetOptimisticStatus::AllButFirst {
                                first_entry: Entry::default(),
                                next_data_set: 11.into(),
                            },
                        ),
                    ]),
                },
            ),
            (
                slot2,
                SlotStatus::Processing {
                    parent_slot: slot2 - 1,
                    verified_up_to: 17.into(),
                    last_data_set: None,
                    data_sets: HashMap::from([
                        (
                            7.into(),
                            DataSetOptimisticStatus::Verified {
                                next_data_set: 13.into(),
                            },
                        ),
                        (
                            13.into(),
                            DataSetOptimisticStatus::Verified {
                                next_data_set: 17.into(),
                            },
                        ),
                    ]),
                },
            ),
        ]);

        let updated = advance_verified(&mut processing_slots, vec![slot1, slot2]);
        assert_eq!(updated, Vec::<Slot>::new());
        assert_eq_sorted!(
            processing_slots,
            HashMap::from([
                (
                    slot1,
                    SlotStatus::Processing {
                        parent_slot: slot1 - 1,
                        verified_up_to: 7.into(),
                        last_data_set: None,
                        data_sets: HashMap::from([
                            (
                                0.into(),
                                DataSetOptimisticStatus::Verified {
                                    next_data_set: 7.into(),
                                },
                            ),
                            (
                                7.into(),
                                DataSetOptimisticStatus::AllButFirst {
                                    first_entry: Entry::default(),
                                    next_data_set: 11.into(),
                                },
                            ),
                        ]),
                    },
                ),
                (
                    slot2,
                    SlotStatus::Processing {
                        parent_slot: slot2 - 1,
                        verified_up_to: 17.into(),
                        last_data_set: None,
                        data_sets: HashMap::from([
                            (
                                7.into(),
                                DataSetOptimisticStatus::Verified {
                                    next_data_set: 13.into(),
                                },
                            ),
                            (
                                13.into(),
                                DataSetOptimisticStatus::Verified {
                                    next_data_set: 17.into(),
                                },
                            ),
                        ]),
                    },
                ),
            ]),
        );
    }
}

mod update_results {
    use {super::*, pretty_assertions_sorted::assert_eq_sorted};

    #[test]
    fn updatd_and_failed() {
        let updated_slot = 3;
        let failed_slot = 7;
        let mut processing_slots = HashMap::from([
            (
                updated_slot,
                SlotStatus::Processing {
                    parent_slot: updated_slot - 1,
                    verified_up_to: 7.into(),
                    last_data_set: None,
                    data_sets: HashMap::from([
                        (
                            0.into(),
                            DataSetOptimisticStatus::Verified {
                                next_data_set: 7.into(),
                            },
                        ),
                        (
                            7.into(),
                            DataSetOptimisticStatus::AllButFirst {
                                first_entry: Entry::default(),
                                next_data_set: 11.into(),
                            },
                        ),
                    ]),
                },
            ),
            (
                failed_slot,
                SlotStatus::Processing {
                    parent_slot: failed_slot - 1,
                    verified_up_to: 17.into(),
                    last_data_set: None,
                    data_sets: HashMap::from([
                        (
                            7.into(),
                            DataSetOptimisticStatus::Verified {
                                next_data_set: 13.into(),
                            },
                        ),
                        (
                            13.into(),
                            DataSetOptimisticStatus::Verified {
                                next_data_set: 17.into(),
                            },
                        ),
                    ]),
                },
            ),
        ]);
        let results = SharedResults::new(Results {
            starting_at: 4,
            slots: HashMap::from([
                (
                    updated_slot,
                    SlotVerificationStatus::Partial {
                        verified_up_to: 4.into(),
                    },
                ),
                (
                    failed_slot,
                    SlotVerificationStatus::Partial {
                        verified_up_to: 7.into(),
                    },
                ),
            ]),
        });

        update_results(
            &mut processing_slots,
            &results,
            vec![updated_slot],
            vec![failed_slot],
        );
        assert_eq_sorted!(
            processing_slots,
            HashMap::from([
                (
                    updated_slot,
                    SlotStatus::Processing {
                        parent_slot: updated_slot - 1,
                        verified_up_to: 7.into(),
                        last_data_set: None,
                        data_sets: HashMap::from([
                            (
                                0.into(),
                                DataSetOptimisticStatus::Verified {
                                    next_data_set: 7.into(),
                                },
                            ),
                            (
                                7.into(),
                                DataSetOptimisticStatus::AllButFirst {
                                    first_entry: Entry::default(),
                                    next_data_set: 11.into(),
                                },
                            ),
                        ]),
                    },
                ),
                (failed_slot, SlotStatus::Failed,),
            ]),
        );

        assert_non_empty_results_in_millis!(
            0,
            results,
            Results {
                starting_at: 4,
                slots: HashMap::from([
                    (
                        updated_slot,
                        SlotVerificationStatus::Partial {
                            verified_up_to: 7.into()
                        }
                    ),
                    (failed_slot, SlotVerificationStatus::Failed),
                ])
            }
        );
    }
}
