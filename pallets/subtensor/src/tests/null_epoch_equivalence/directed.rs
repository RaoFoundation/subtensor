//! States that are built by hand: each one exercises a named part of the Null epoch.

use super::*;

/// A Null subnet with `n` registered hotkeys and `mechanisms` mechanisms at block 10 000.
/// Storage is written directly, so `n` can be the full UID budget without `n` extrinsics.
fn null_subnet(n: u16, mechanisms: u8) -> NetUid {
    null_subnet_at(NetUid::from(1), n, mechanisms)
}

fn null_subnet_at(netuid: NetUid, n: u16, mechanisms: u8) -> NetUid {
    System::set_block_number(10_000);
    add_network_disable_commit_reveal(netuid, 360, 0);
    SubtensorModule::set_epoch_consensus(netuid, EpochConsensus::Null);
    SubtensorModule::set_max_allowed_uids(netuid, NULL_UID_BUDGET);
    SubtensorModule::set_max_allowed_validators(netuid, 1);
    StakeThreshold::<Test>::put(0);
    for uid in 0..n {
        Keys::<Test>::insert(netuid, uid, hotkey(uid));
        Uids::<Test>::insert(netuid, hotkey(uid), uid);
        IsNetworkMember::<Test>::insert(hotkey(uid), netuid, true);
        BlockAtRegistration::<Test>::insert(netuid, uid, 100);
    }
    SubnetworkN::<Test>::insert(netuid, n);
    ValidatorPermit::<Test>::insert(netuid, vec![false; usize::from(n)]);
    MaxMechanismCount::<Test>::put(MechId::from(MAX_MECHANISM_COUNT_PER_SUBNET));
    MechanismCountCurrent::<Test>::insert(netuid, MechId::from(mechanisms));
    for mechanism in 0..mechanisms {
        LastUpdate::<Test>::insert(
            mechanism_index(netuid, mechanism),
            vec![9_990u64; usize::from(n)],
        );
    }
    netuid
}

/// A row over the first `len` UIDs with varied weights, in a fixed scrambled order.
fn scrambled_row(len: u16, salt: u16) -> Vec<(u16, u16)> {
    let mut row: Vec<(u16, u16)> = (0..len)
        .map(|uid| (uid, 1 + (uid.wrapping_mul(7919) ^ salt) % 5000))
        .collect();
    let mut rng = Rng(u64::from(salt) + 1);
    for last in (1..row.len()).rev() {
        row.swap(last, rng.below(last as u64 + 1) as usize);
    }
    row
}

#[test]
fn shared_null_epoch_matches_reference_for_every_mechanism_count_and_population() {
    for mechanisms in 1..=MAX_MECHANISM_COUNT_PER_SUBNET {
        for n in [1u16, 2, 3, 16, 255, 256, 257, 1_000, NULL_UID_BUDGET] {
            // The full budget with every count repeats 2 500-UID epochs 136 times.
            if n > 257 && ![1, 2, 7, 16].contains(&mechanisms) {
                continue;
            }
            new_test_ext(1).execute_with(|| {
                let netuid = null_subnet(n, mechanisms);
                for uid in 0..n {
                    set_stake(netuid, uid, 1_000_000 + 37 * u64::from(n - uid));
                }
                for mechanism in 0..mechanisms {
                    // Rows as long as each mechanism index ever admitted.
                    let len = n.min(TOTAL_NULL_WEIGHT_BUDGET / (u16::from(mechanism) + 1));
                    Weights::<Test>::insert(
                        mechanism_index(netuid, mechanism),
                        0,
                        scrambled_row(len, u16::from(mechanism)),
                    );
                }
                let outcome = assert_equivalent(netuid, 1_000_000_007, 600_000_011, 400_000_003);
                assert_eq!(outcome.emissions.len(), usize::from(n));
                assert_eq!(paid(&outcome), (1_000_000_007, 1_000_000_014));
                for mechanism in 0..mechanisms {
                    // UID 0 is the validator: only a second UID gives it a miner to weight.
                    assert_eq!(paid_by_weights(&outcome, mechanism), n > 1, "n={n}");
                }
            });
        }
    }
}

#[test]
fn shared_null_epoch_matches_reference_for_empty_and_masked_rows() {
    new_test_ext(1).execute_with(|| {
        let netuid = null_subnet(40, 6);
        for uid in 0..40 {
            set_stake(netuid, uid, 500 + u64::from(uid));
        }
        // UID 39 has the highest stake and holds every row below.
        let row = |targets: &[u16]| targets.iter().map(|uid| (*uid, 7u16)).collect::<Vec<_>>();
        // Mechanism 0: no row. Mechanism 1: an empty row.
        Weights::<Test>::insert(mechanism_index(netuid, 1), 39, Vec::<(u16, u16)>::new());
        // Mechanism 2: only a self-weight, which a validator that is not the owner loses.
        Weights::<Test>::insert(mechanism_index(netuid, 2), 39, row(&[39]));
        // Mechanism 3: every weight is zero.
        Weights::<Test>::insert(mechanism_index(netuid, 3), 39, vec![(1, 0), (2, 0)]);
        // Mechanism 4: every target registered after the row's last update.
        Weights::<Test>::insert(mechanism_index(netuid, 4), 39, row(&[5, 6]));
        BlockAtRegistration::<Test>::insert(netuid, 5, 9_990);
        BlockAtRegistration::<Test>::insert(netuid, 6, 9_995);
        // Mechanism 5: one target survives.
        Weights::<Test>::insert(mechanism_index(netuid, 5), 39, row(&[5, 7]));
        // Rows of other UIDs must not count.
        Weights::<Test>::insert(mechanism_index(netuid, 0), 38, row(&[1, 2, 3]));

        let outcome = assert_equivalent(netuid, 6_000_007, 3_000_001, 999_983);
        for mechanism in 0..5 {
            assert!(!paid_by_weights(&outcome, mechanism), "{mechanism}");
        }
        assert!(paid_by_weights(&outcome, 5));
        assert_eq!(paid(&outcome), (6_000_007, 3_999_984));
    });
}

#[test]
fn shared_null_epoch_matches_reference_for_stake_shapes() {
    // (stakes by UID, expected validator)
    let cases: [(&[u64], u16); 6] = [
        // A single staker.
        (&[0, 0, 9, 0, 0], 2),
        // A tie selects the first UID.
        (&[5, 5, 5, 5, 5], 0),
        (&[1, 7, 7, 3, 7], 1),
        // Stake that differs by one unit.
        (&[1_000_000_000, 1_000_000_001, 999_999_999, 0, 1], 1),
        // Zero stake: no validator has a positive stake, UID 0 holds the permit.
        (&[0, 0, 0, 0, 0], 0),
        (&[u64::MAX / 4, 1, 0, u64::MAX / 8, 3], 0),
    ];
    for (stakes, validator) in cases {
        for mechanisms in [1u8, 3, 16] {
            new_test_ext(1).execute_with(|| {
                let n = stakes.len() as u16;
                let netuid = null_subnet(n, mechanisms);
                for (uid, alpha) in stakes.iter().enumerate() {
                    set_stake(netuid, uid as u16, *alpha);
                }
                for mechanism in 0..mechanisms {
                    // Every UID has a row: only the validator's row may count.
                    for uid in 0..n {
                        Weights::<Test>::insert(
                            mechanism_index(netuid, mechanism),
                            uid,
                            vec![
                                ((uid + 1) % n, 3),
                                ((uid + 2) % n, 1 + u16::from(mechanism)),
                            ],
                        );
                    }
                }
                let outcome = assert_equivalent(netuid, 70_001, 20_003, 10_007);
                let permits = &outcome.storage.validator_permit;
                assert_eq!(
                    permits.iter().position(|permit| *permit),
                    Some(usize::from(validator)),
                    "{stakes:?}"
                );
                let funded = stakes.iter().any(|alpha| *alpha > 0);
                // Without stake, the local dividend budget goes to miners and root is not paid.
                let expected = if funded {
                    (70_001, 30_010)
                } else {
                    (90_004, 0)
                };
                assert_eq!(paid(&outcome), expected, "{stakes:?}");
            });
        }
    }
}

#[test]
fn shared_null_epoch_matches_reference_for_inactive_validators() {
    new_test_ext(1).execute_with(|| {
        let netuid = null_subnet(12, 4);
        for uid in 0..12 {
            set_stake(netuid, uid, 100 + u64::from(uid));
        }
        let cutoff = SubtensorModule::get_activity_cutoff_blocks(netuid);
        let now = SubtensorModule::get_current_block_as_u64();
        // Mechanism 0: every UID is inactive. Mechanism 1: the boundary blocks.
        // Mechanism 2: only UID 3 is active. Mechanism 3: a short vector.
        LastUpdate::<Test>::insert(mechanism_index(netuid, 0), vec![0u64; 12]);
        LastUpdate::<Test>::insert(
            mechanism_index(netuid, 1),
            (0..12u64)
                .map(|uid| now - cutoff - 2 + uid % 4)
                .collect::<Vec<_>>(),
        );
        let mut one_active = vec![1u64; 12];
        one_active[3] = now;
        LastUpdate::<Test>::insert(mechanism_index(netuid, 2), one_active);
        LastUpdate::<Test>::insert(mechanism_index(netuid, 3), vec![now; 5]);
        // The validator (UID 11) is inactive on mechanism 0 and still earns and directs rewards.
        for mechanism in 0..4 {
            Weights::<Test>::insert(mechanism_index(netuid, mechanism), 11, vec![(1, 1), (2, 2)]);
        }
        BlockAtRegistration::<Test>::insert(netuid, 1, 0);
        BlockAtRegistration::<Test>::insert(netuid, 2, 0);

        let outcome = assert_equivalent(netuid, 10_000, 5_000, 2_500);
        // A UID is active when it is active on at least one mechanism.
        let active = &outcome.storage.active;
        assert!(active[3] && active[0] && active[4]);
        assert!(active.iter().any(|flag| !*flag));
        assert_eq!(paid(&outcome), (10_000, 7_500));
    });
}

/// A Null subnet built through registrations, so that weight extrinsics pass their checks.
/// UID 0 holds the stake and the validator permit.
fn registered_null_subnet(n: u16, mechanisms: u8) -> NetUid {
    let netuid = NetUid::from(1);
    add_network_disable_commit_reveal(netuid, 360, 0);
    SubtensorModule::set_max_allowed_uids(netuid, NULL_UID_BUDGET);
    SubtensorModule::set_weights_set_rate_limit(netuid, 0);
    for uid in 0..n {
        SubtensorModule::append_neuron(netuid, &hotkey(uid), 0);
    }
    SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
        &hotkey(0),
        &hotkey(0),
        netuid,
        1_000_000_000u64.into(),
    );
    SubtensorModule::set_validator_permit_for_uid(netuid, 0, true);
    System::set_block_number(1);
    assert_ok!(SubtensorModule::do_set_epoch_consensus(
        netuid,
        EpochConsensus::Null
    ));
    if mechanisms != 1 {
        assert_ok!(SubtensorModule::do_set_max_mechanism_count(
            mechanisms.into()
        ));
        assert_ok!(SubtensorModule::do_set_mechanism_count(
            netuid,
            mechanisms.into()
        ));
    }
    netuid
}

#[test]
fn shared_null_epoch_matches_reference_for_delegated_mechanism_weight_setters() {
    new_test_ext(1).execute_with(|| {
        let mechanisms = MAX_MECHANISM_COUNT_PER_SUBNET;
        let netuid = registered_null_subnet(200, mechanisms);
        let cap = SubtensorModule::max_null_weights_per_row(netuid);
        assert_eq!(cap, 156);
        System::set_block_number(50);
        for mechanism in 0..mechanisms {
            let mecid = MechId::from(mechanism);
            // Mechanism 15 keeps no delegate and no row.
            if mechanism == 15 {
                continue;
            }
            let (uids, values): (Vec<u16>, Vec<u16>) = (1..=cap.min(10 * u16::from(mechanism) + 1))
                .map(|uid| (uid, uid + u16::from(mechanism)))
                .unzip();
            // Odd mechanisms: a delegate writes the validator's row. Even: the validator does.
            if mechanism % 2 == 1 {
                let delegate = U256::from(77_000 + u64::from(mechanism));
                assert_ok!(SubtensorModule::set_mech_weight_delegate(
                    RuntimeOrigin::signed(hotkey(0)),
                    netuid,
                    mecid,
                    Some(delegate),
                ));
                assert_ok!(SubtensorModule::set_mechanism_weights_for(
                    RuntimeOrigin::signed(delegate),
                    netuid,
                    mecid,
                    hotkey(0),
                    uids,
                    values,
                    0,
                ));
            } else {
                assert_ok!(SubtensorModule::set_mechanism_weights(
                    RuntimeOrigin::signed(hotkey(0)),
                    netuid,
                    mecid,
                    uids,
                    values,
                    0,
                ));
            }
        }
        System::set_block_number(60);
        let outcome = assert_equivalent(netuid, 1_000_000, 400_000, 100_000);
        for mechanism in 0..15 {
            assert!(paid_by_weights(&outcome, mechanism), "{mechanism}");
        }
        assert!(!paid_by_weights(&outcome, 15));
        assert_eq!(paid(&outcome), (1_000_000, 500_000));
    });
}

#[test]
fn shared_null_epoch_matches_reference_with_commit_reveal() {
    new_test_ext(1).execute_with(|| {
        let mechanisms = 4u8;
        let netuid = registered_null_subnet(30, mechanisms);
        System::set_block_number(20);
        // Rows that exist before commit-reveal is enabled.
        for mechanism in 0..mechanisms {
            assert_ok!(SubtensorModule::set_mechanism_weights(
                RuntimeOrigin::signed(hotkey(0)),
                netuid,
                MechId::from(mechanism),
                (1..12).collect(),
                (1..12).map(|uid| uid * 3 + u16::from(mechanism)).collect(),
                0,
            ));
        }
        SubtensorModule::set_commit_reveal_weights_enabled(netuid, true);
        assert_ok!(SubtensorModule::set_reveal_period(netuid, 1));
        System::set_block_number(40);

        // Mechanism 0: a pending hash commit of the validator.
        let salt = vec![1u16, 2, 3];
        let (uids, values): (Vec<u16>, Vec<u16>) = (vec![2, 3, 4], vec![9, 8, 7]);
        let hash = |mechanism: u8| {
            SubtensorModule::get_commit_hash(
                &hotkey(0),
                mechanism_index(netuid, mechanism),
                &uids,
                &values,
                &salt,
                0,
            )
        };
        assert_ok!(SubtensorModule::commit_mechanism_weights(
            RuntimeOrigin::signed(hotkey(0)),
            netuid,
            MechId::from(0),
            hash(0),
        ));
        // Mechanism 1: a delegate commits and reveals for the validator.
        let delegate = U256::from(88_000);
        assert_ok!(SubtensorModule::set_mech_weight_delegate(
            RuntimeOrigin::signed(hotkey(0)),
            netuid,
            MechId::from(1),
            Some(delegate),
        ));
        assert_ok!(SubtensorModule::commit_mechanism_weights_for(
            RuntimeOrigin::signed(delegate),
            netuid,
            MechId::from(1),
            hotkey(0),
            hash(1),
        ));
        let epoch = SubtensorModule::current_epoch_with_lookahead(netuid);
        // Mechanism 2: a pending timelocked commit of the validator and one of a miner.
        TimelockedWeightCommits::<Test>::insert(
            mechanism_index(netuid, 2),
            epoch,
            VecDeque::from([
                (hotkey(0), 41u64, vec![1u8; 9].try_into().unwrap(), 1_000u64),
                (hotkey(5), 3u64, vec![2u8; 9].try_into().unwrap(), 1_000u64),
            ]),
        );
        // Mechanism 3: only an expired commit, which must not mask anything.
        WeightCommits::<Test>::insert(
            mechanism_index(netuid, 3),
            hotkey(0),
            VecDeque::from([(H256::repeat_byte(3), 0u64, 1u64, 0u64)]),
        );

        // UIDs 2 and 9 are replaced after the commits: their columns are masked where a
        // commit is pending.
        System::set_block_number(45);
        SubtensorModule::replace_neuron(netuid, 2, &U256::from(500_002), 45);
        SubtensorModule::replace_neuron(netuid, 9, &U256::from(500_009), 45);
        System::set_block_number(50);
        SubnetEpochIndex::<Test>::insert(netuid, epoch + 1);
        assert_equivalent(netuid, 9_000_000, 600_000, 400_000);

        // The delegate reveals on mechanism 1: the row changes and the commit is gone.
        assert_ok!(SubtensorModule::reveal_mechanism_weights_for(
            RuntimeOrigin::signed(delegate),
            netuid,
            MechId::from(1),
            hotkey(0),
            uids.clone(),
            values.clone(),
            salt.clone(),
            0,
        ));
        assert_eq!(
            Weights::<Test>::get(mechanism_index(netuid, 1), 0),
            vec![(2, 9), (3, 8), (4, 7)]
        );
        let outcome = assert_equivalent(netuid, 9_000_000, 600_000, 400_000);
        assert_eq!(paid(&outcome), (9_000_000, 1_000_000));

        // Later epochs: the remaining commits expire.
        SubnetEpochIndex::<Test>::insert(netuid, epoch + 5);
        assert_equivalent(netuid, 9_000_000, 600_000, 400_000);
    });
}

#[test]
fn shared_null_epoch_matches_reference_for_oversized_legacy_rows() {
    new_test_ext(1).execute_with(|| {
        // A full row written while the subnet had one mechanism.
        let netuid = registered_null_subnet(NULL_UID_BUDGET, 1);
        let full: Vec<u16> = (0..NULL_UID_BUDGET).collect();
        assert_ok!(SubtensorModule::set_weights(
            RuntimeOrigin::signed(hotkey(0)),
            netuid,
            full.clone(),
            full.iter().map(|uid| uid % 97 + 1).collect(),
            0,
        ));
        for (count, len) in [(2u8, 1_250u16), (4, 625), (16, 156)] {
            assert_ok!(SubtensorModule::do_set_max_mechanism_count(count.into()));
            assert_ok!(SubtensorModule::do_set_mechanism_count(
                netuid,
                count.into()
            ));
            assert_eq!(SubtensorModule::max_null_weights_per_row(netuid), len);
            // The newest mechanism takes the longest row that the new count admits.
            assert_ok!(SubtensorModule::set_mechanism_weights(
                RuntimeOrigin::signed(hotkey(0)),
                netuid,
                MechId::from(count - 1),
                (1..=len).collect(),
                (1..=len).map(|uid| uid % 13 + 1).collect(),
                0,
            ));
        }
        // The rows written under smaller counts stay longer than the current cap.
        assert_eq!(
            Weights::<Test>::get(mechanism_index(netuid, 0), 0).len(),
            usize::from(NULL_UID_BUDGET)
        );
        assert_eq!(
            Weights::<Test>::get(mechanism_index(netuid, 1), 0).len(),
            1_250
        );
        // Rows that no extrinsic admits: longer than the population, with repeated and
        // unregistered targets.
        let mut oversized: Vec<(u16, u16)> = (0..6_000u16)
            .map(|entry| (entry % 3_000, entry % 11))
            .collect();
        oversized.push((u16::MAX, u16::MAX));
        Weights::<Test>::insert(mechanism_index(netuid, 5), 0, oversized);
        SubtensorModule::set_last_update_for_uid(mechanism_index(netuid, 5), 0, 1);

        let outcome = assert_equivalent(netuid, 123_456_789, 55_555, 44_444);
        for mechanism in [0u8, 1, 3, 5, 15] {
            assert!(paid_by_weights(&outcome, mechanism), "{mechanism}");
        }
        assert_eq!(paid(&outcome), (123_456_789, 99_999));
    });
}

#[test]
fn shared_null_epoch_matches_reference_for_emission_splits_and_extreme_budgets() {
    let splits: [Option<Vec<u16>>; 5] = [
        None,
        // Every mechanism has its own share.
        Some(vec![1, 2, 3, 4, 65_525]),
        // Equal shares, and mechanisms without a budget.
        Some(vec![0, 30_000, 0, 30_000, 5_535]),
        Some(vec![65_535, 0, 0, 0, 0]),
        // A malformed split has the wrong length: the epoch splits evenly.
        Some(vec![65_535]),
    ];
    let budgets: [(u64, u64, u64); 9] = [
        (0, 0, 0),
        (1, 0, 0),
        (0, 1, 0),
        (0, 0, 1),
        (3, 2, 1),
        (1_000_003, 500_009, 250_007),
        (u64::MAX, 0, 0),
        (u64::MAX, u64::MAX, u64::MAX),
        (u64::MAX - 300, 100, u64::MAX / 2),
    ];
    for split in &splits {
        for (miner, local, root) in budgets {
            for staked in [true, false] {
                new_test_ext(1).execute_with(|| {
                    let netuid = null_subnet(9, 5);
                    if staked {
                        for uid in 0..9 {
                            set_stake(netuid, uid, 1 + u64::from(uid) * 1_000_003);
                        }
                    }
                    if let Some(split) = split {
                        MechanismEmissionSplit::<Test>::insert(netuid, split);
                    }
                    let validator = if staked { 8 } else { 0 };
                    for mechanism in [0u8, 2, 3] {
                        Weights::<Test>::insert(
                            mechanism_index(netuid, mechanism),
                            validator,
                            vec![(1, u16::MAX), (2, 1), (3, 30_000 + u16::from(mechanism))],
                        );
                    }
                    assert_equivalent(netuid, miner, local, root);
                });
            }
        }
    }
}

#[test]
fn shared_null_epoch_matches_reference_when_sums_reach_the_largest_balance() {
    for mechanisms in [1u8, 2, 16] {
        new_test_ext(1).execute_with(|| {
            let netuid = null_subnet(4, mechanisms);
            // UID 3 is the validator and the owner, so it keeps its self-weight. Its stake
            // summed over 16 mechanisms is more than the largest balance.
            set_stake(netuid, 3, 1 << 62);
            set_stake(netuid, 1, 5);
            SubnetOwnerHotkey::<Test>::insert(netuid, hotkey(3));
            for mechanism in 0..mechanisms {
                Weights::<Test>::insert(
                    mechanism_index(netuid, mechanism),
                    3,
                    vec![(3, u16::MAX), (1, 1)],
                );
            }
            // Every pool holds the largest balance: the dividend budget of each mechanism,
            // the dividends of UID 3 and its miner payout plus dividends all reach the cap.
            let outcome = assert_equivalent(netuid, u64::MAX, u64::MAX, u64::MAX);
            let (_, miner, validator) = outcome
                .emissions
                .iter()
                .find(|(who, _, _)| *who == hotkey(3))
                .unwrap();
            assert!(u64::from(*miner) > u64::MAX / 2);
            if mechanisms > 1 {
                assert_eq!(u64::from(*validator), u64::MAX);
            }
            assert_eq!(u64::from(outcome.storage.emission[3]), u64::MAX);
        });
    }
}

#[test]
fn shared_null_epoch_matches_reference_for_owner_self_weight_and_stake_threshold() {
    for owner in [None, Some(0u16), Some(6), Some(3)] {
        for threshold in [0u64, 500, u64::MAX] {
            new_test_ext(1).execute_with(|| {
                let netuid = null_subnet(7, 3);
                for uid in 0..7 {
                    set_stake(netuid, uid, 100 * u64::from(uid));
                }
                StakeThreshold::<Test>::put(threshold);
                match owner {
                    Some(uid) => SubnetOwnerHotkey::<Test>::insert(netuid, hotkey(uid)),
                    None => SubnetOwnerHotkey::<Test>::insert(netuid, U256::from(1)),
                }
                // UID 6 is the validator. It keeps its self-weight only as the owner.
                for mechanism in 0..3 {
                    Weights::<Test>::insert(
                        mechanism_index(netuid, mechanism),
                        6,
                        vec![(6, 50), (1, 10 + u16::from(mechanism))],
                    );
                }
                let outcome = assert_equivalent(netuid, 50_000, 20_000, 10_000);
                let self_weight = outcome.storage.incentive[0][6] != PerU16::zero();
                assert_eq!(self_weight, owner == Some(6));
            });
        }
    }
}

#[test]
fn shared_null_epoch_matches_reference_for_inconsistent_storage() {
    // States that registration cannot produce. The reference defines their result.
    let cases: [(&str, fn(NetUid)); 9] = [
        ("a UID without a hotkey", |netuid| {
            #[allow(unknown_lints)]
            Keys::<Test>::remove(netuid, 4);
        }),
        ("the validator's UID without a hotkey", |netuid| {
            #[allow(unknown_lints)]
            Keys::<Test>::remove(netuid, 9);
        }),
        ("hotkeys above the UID count", |netuid| {
            Keys::<Test>::insert(netuid, 10, U256::from(77));
            Keys::<Test>::insert(netuid, 300, U256::from(78));
            TotalHotkeyAlpha::<Test>::insert(
                U256::from(78),
                netuid,
                AlphaBalance::from(1u64 << 40),
            );
        }),
        ("one hotkey on two UIDs", |netuid| {
            Keys::<Test>::insert(netuid, 2, hotkey(7));
        }),
        ("the validator's hotkey on two UIDs", |netuid| {
            Keys::<Test>::insert(netuid, 256, hotkey(9));
            Keys::<Test>::insert(netuid, 3, hotkey(9));
        }),
        ("no mechanism", |netuid| {
            MechanismCountCurrent::<Test>::insert(netuid, MechId::from(0));
        }),
        ("more mechanisms than storage indices", |netuid| {
            MechanismCountCurrent::<Test>::insert(netuid, MechId::from(19));
        }),
        ("update vectors of other lengths", |netuid| {
            LastUpdate::<Test>::insert(mechanism_index(netuid, 0), Vec::<u64>::new());
            LastUpdate::<Test>::insert(mechanism_index(netuid, 1), vec![9_999u64; 400]);
            LastUpdate::<Test>::insert(mechanism_index(netuid, 2), vec![9_999u64; 3]);
        }),
        ("a UID count above the hotkeys", |netuid| {
            SubnetworkN::<Test>::insert(netuid, 14);
        }),
    ];
    for (name, corrupt) in cases {
        for commit_reveal in [false, true] {
            new_test_ext(1).execute_with(|| {
                let netuid = null_subnet(10, 3);
                for uid in 0..10 {
                    set_stake(netuid, uid, 10 + u64::from(uid));
                }
                for mechanism in 0..3 {
                    Weights::<Test>::insert(
                        mechanism_index(netuid, mechanism),
                        9,
                        vec![(4, 5), (2, 6), (7, 7), (10, 8), (12, 9)],
                    );
                    WeightCommits::<Test>::insert(
                        mechanism_index(netuid, mechanism),
                        hotkey(9),
                        VecDeque::from([(H256::zero(), 0u64, 50u64, 0u64)]),
                    );
                }
                SubtensorModule::set_commit_reveal_weights_enabled(netuid, commit_reveal);
                corrupt(netuid);
                println!("{name}, commit reveal {commit_reveal}");
                assert_equivalent(netuid, 1_000, 500, 250);
            });
        }
    }
}

#[test]
fn shared_null_epoch_matches_reference_above_the_storage_index_range() {
    // No subnet has a netuid of 4 096 or more. If one had, its mechanism storage indices
    // would name other subnets, and its update vectors would take another subnet's length.
    for netuid in [4_096u16, 4_103, 9_000, u16::MAX] {
        new_test_ext(1).execute_with(|| {
            let netuid = null_subnet_at(NetUid::from(netuid), 6, 3);
            for uid in 0..6 {
                set_stake(netuid, uid, 10 + u64::from(uid));
            }
            // UID 5 is the validator. A short update vector does not reach its UID.
            LastUpdate::<Test>::insert(mechanism_index(netuid, 0), vec![9_999u64; 2]);
            LastUpdate::<Test>::insert(mechanism_index(netuid, 1), vec![9_999u64; 6]);
            LastUpdate::<Test>::remove(mechanism_index(netuid, 2));
            for mechanism in 0..3 {
                Weights::<Test>::insert(
                    mechanism_index(netuid, mechanism),
                    5,
                    vec![(1, 5), (2, 6)],
                );
            }
            let outcome = assert_equivalent(netuid, 1_000, 500, 250);
            // At the largest netuid, every mechanism has the same saturated storage index.
            if u16::from(netuid) < u16::MAX {
                // A row counts only where the update vector holds the validator's block.
                assert!(!paid_by_weights(&outcome, 0));
                assert!(paid_by_weights(&outcome, 1));
                assert!(!paid_by_weights(&outcome, 2));
            }
        });
    }
}

#[test]
fn shared_null_epoch_matches_reference_with_voting_power_tracking() {
    for disable_at in [0u64, 9_999, 10_000, 20_000] {
        new_test_ext(1).execute_with(|| {
            let netuid = null_subnet(8, 16);
            for uid in 0..8 {
                set_stake(netuid, uid, u64::MAX / 40 + u64::from(uid));
            }
            VotingPowerTrackingEnabled::<Test>::insert(netuid, true);
            if disable_at > 0 {
                VotingPowerDisableAtBlock::<Test>::insert(netuid, disable_at);
            }
            // A miner, the validator and a deregistered hotkey hold voting power.
            VotingPower::<Test>::insert(netuid, hotkey(1), 600u64);
            VotingPower::<Test>::insert(netuid, hotkey(7), 3_000u64);
            VotingPower::<Test>::insert(netuid, U256::from(5), 40u64);
            TotalVotingPower::<Test>::insert(netuid, 3_640u64);
            let outcome = assert_equivalent(netuid, 1_000, 500, 250);
            let holders: Vec<U256> = outcome
                .storage
                .voting_power
                .iter()
                .map(|(who, _)| *who)
                .collect();
            if disable_at == 0 || disable_at > 10_000 {
                // Only the elected validator keeps voting power. Its stake term is the
                // sum over the 16 mechanisms, which saturates here.
                assert_eq!(holders, vec![hotkey(7)]);
                assert!(outcome.storage.voting_power[0].1 > 3_000);
            } else {
                // The grace period is over: tracking stops and every entry is cleared.
                assert!(holders.is_empty());
            }
        });
    }
}

#[test]
fn yuma_epoch_still_runs_the_per_mechanism_path() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        add_network_disable_commit_reveal(netuid, 360, 0);
        SubtensorModule::set_max_allowed_uids(netuid, 64);
        SubtensorModule::set_weights_set_rate_limit(netuid, 0);
        for uid in 0..8u16 {
            SubtensorModule::append_neuron(netuid, &hotkey(uid), 0);
            SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey(uid),
                &hotkey(uid),
                netuid,
                (1_000_000_000u64 * (u64::from(uid) + 1)).into(),
            );
            SubtensorModule::set_validator_permit_for_uid(netuid, uid, uid < 3);
        }
        assert_ok!(SubtensorModule::do_set_mechanism_count(netuid, 2.into()));
        System::set_block_number(5);
        for uid in 0..3u16 {
            for mechanism in 0..2u8 {
                assert_ok!(SubtensorModule::set_mechanism_weights(
                    RuntimeOrigin::signed(hotkey(uid)),
                    netuid,
                    MechId::from(mechanism),
                    vec![4, 5, 6],
                    vec![10 + uid, 20, 30 + u16::from(mechanism)],
                    0,
                ));
            }
        }
        System::set_block_number(10);
        assert_eq!(
            SubtensorModule::get_epoch_consensus(netuid),
            EpochConsensus::Yuma
        );
        let budgets = (5_000_000u64, 3_000_000u64, 1_000_000u64);
        let per_mechanism = outcome_of(netuid, || {
            SubtensorModule::epoch_per_mechanism_with_budgets(
                netuid,
                budgets.0.into(),
                budgets.1.into(),
                budgets.2.into(),
            )
        });
        let entry_point = outcome_of(netuid, || {
            SubtensorModule::epoch_with_mechanism_budgets(
                netuid,
                budgets.0.into(),
                budgets.1.into(),
                budgets.2.into(),
            )
        });
        assert_eq!(entry_point, per_mechanism);
        assert!(paid(&per_mechanism).0 > 0);
        // Yuma wrote bonds on both mechanisms.
        assert!(per_mechanism.events.len() >= 2);
    });
}

#[test]
fn shared_null_epoch_matches_reference_across_consecutive_epochs() {
    new_test_ext(1).execute_with(|| {
        let mechanisms = 5u8;
        let n = 48u16;
        let netuid = registered_null_subnet(n, mechanisms);
        SubnetOwner::<Test>::insert(netuid, U256::from(999_999));
        SubnetAlphaOut::<Test>::insert(netuid, AlphaBalance::from(1_000_000_000_000_000u64));
        let mut rng = Rng(0xE90C);
        let mut rows_written = 0u32;
        let mut validators = alloc::collections::BTreeSet::new();
        for round in 0..40u64 {
            System::set_block_number(System::block_number() + 1 + rng.below(700));
            // The permit holder of the previous epoch writes rows on some mechanisms.
            let holder = ValidatorPermit::<Test>::get(netuid)
                .iter()
                .position(|permit| *permit)
                .map(|uid| uid as u16);
            if let Some(holder) = holder {
                validators.insert(holder);
                for mechanism in 0..mechanisms {
                    if rng.chance(55) {
                        continue;
                    }
                    let len = rng.range(1, 30) as u16;
                    let first = rng.below(u64::from(n)) as u16;
                    let uids: Vec<u16> = (0..len)
                        .map(|step| (first + step) % n)
                        .filter(|uid| *uid != holder)
                        .collect();
                    let values: Vec<u16> =
                        uids.iter().map(|_| rng.range(1, 60_000) as u16).collect();
                    if SubtensorModule::set_mechanism_weights(
                        RuntimeOrigin::signed(Keys::<Test>::get(netuid, holder)),
                        netuid,
                        MechId::from(mechanism),
                        uids,
                        values,
                        0,
                    )
                    .is_ok()
                    {
                        rows_written += 1;
                    }
                }
            }
            // Stake moves, so that the validator changes between epochs.
            for _ in 0..rng.below(4) {
                let staker = Keys::<Test>::get(netuid, rng.below(u64::from(n)) as u16);
                SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
                    &staker,
                    &staker,
                    netuid,
                    rng.range(1, 900_000_000).into(),
                );
            }
            // A registration replaces a UID: rows that are older than it lose that target.
            if rng.chance(40) {
                let uid = rng.below(u64::from(n)) as u16;
                SubtensorModule::replace_neuron(
                    netuid,
                    uid,
                    &U256::from(1_000_000 + round),
                    System::block_number(),
                );
            }
            let budgets = (
                rng.range(0, 5_000_000_000),
                rng.range(0, 3_000_000_000),
                rng.range(0, 1_000_000_000),
            );
            assert_equivalent(netuid, budgets.0, budgets.1, budgets.2);
            // The real epoch and its settlement produce the state of the next round.
            SubtensorModule::distribute_emission(
                netuid,
                budgets.0.into(),
                budgets.1.into(),
                budgets.2.into(),
                0u64.into(),
            );
            SubnetEpochIndex::<Test>::mutate(netuid, |index| *index += 1);
        }
        assert!(rows_written > 20, "{rows_written}");
        assert!(validators.len() > 1, "{validators:?}");
    });
}
