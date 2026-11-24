//! Random Null subnets, each built from a seed.

use super::*;

#[derive(Clone, Copy)]
struct Shape {
    max_uids: u16,
    /// Also build states that no extrinsic can reach.
    inconsistent: bool,
}

struct Case {
    netuid: NetUid,
    uids: u16,
    mechanisms: u8,
    budgets: (u64, u64, u64),
}

fn random_weight(rng: &mut Rng) -> u16 {
    match rng.below(10) {
        0 => 0,
        1 => 1,
        2 => u16::MAX,
        _ => rng.range(1, 65_535) as u16,
    }
}

/// A stored row for a subnet of `n` UIDs. `owner_of_row` is the UID that holds the row.
fn random_row(rng: &mut Rng, n: u16, owner_of_row: u16, shape: Shape) -> Vec<(u16, u16)> {
    let uid = |rng: &mut Rng| rng.below(u64::from(n.max(1))) as u16;
    match rng.below(14) {
        0 => Vec::new(),
        1 => vec![(owner_of_row, random_weight(rng))],
        2 => (0..rng.range(1, 8)).map(|_| (uid(rng), 0)).collect(),
        3 => vec![(uid(rng), random_weight(rng))],
        4 => {
            // Every UID, in a random order.
            let mut row: Vec<(u16, u16)> = (0..n).map(|uid| (uid, random_weight(rng))).collect();
            for last in (1..row.len()).rev() {
                row.swap(last, rng.below(last as u64 + 1) as usize);
            }
            row
        }
        5 if shape.inconsistent => {
            // Repeated targets: the last entry of each UID must win.
            (0..rng.range(2, 40))
                .map(|_| (uid(rng) % 5, random_weight(rng)))
                .collect()
        }
        6 if shape.inconsistent => {
            // Targets above the UID count, and more entries than UIDs.
            (0..rng.range(1, 2 * u64::from(n) + 4))
                .map(|_| (rng.below(u64::from(n) + 6) as u16, random_weight(rng)))
                .collect()
        }
        _ => {
            let mut row: Vec<(u16, u16)> = Vec::new();
            for _ in 0..rng.range(1, u64::from(n.clamp(1, 60))) {
                let target = uid(rng);
                if row.iter().all(|(seen, _)| *seen != target) {
                    row.push((target, random_weight(rng)));
                }
            }
            row
        }
    }
}

/// A random split of 65 535 into `parts` shares. Shares can be zero or equal.
fn random_split(rng: &mut Rng, parts: usize) -> Vec<u16> {
    let mut cuts: Vec<u64> = (1..parts)
        .map(|_| match rng.below(4) {
            0 => rng.pick(&[0u64, 21_845, 43_690, 65_535]),
            _ => rng.range(0, 65_535),
        })
        .collect();
    cuts.sort_unstable();
    cuts.push(65_535);
    let mut previous = 0;
    cuts.into_iter()
        .map(|cut| {
            let share = (cut - previous) as u16;
            previous = cut;
            share
        })
        .collect()
}

/// Builds one random Null subnet in the current externalities.
fn random_case(rng: &mut Rng, shape: Shape) -> Case {
    let netuid = NetUid::from(rng.range(1, 250) as u16);
    let block = rng.range(1, 3_000_000);
    System::set_block_number(block);
    let tempo = rng.pick(&[1u16, 2, 13, 360, 360, 7_200, 50_400]);
    add_network_disable_commit_reveal(netuid, tempo, 0);
    SubtensorModule::set_epoch_consensus(netuid, EpochConsensus::Null);
    SubtensorModule::set_max_allowed_validators(netuid, 1);
    ActivityCutoffFactorMilli::<Test>::insert(netuid, rng.pick(&[1_000u32, 13_889, 50_000]));
    SubtensorModule::set_tao_weight(rng.pick(&[0u64, u64::MAX / 6, u64::MAX / 2, u64::MAX]));
    LastEpochBlock::<Test>::insert(netuid, block - rng.below(2 * u64::from(tempo)).min(block));
    let cutoff = SubtensorModule::get_activity_cutoff_blocks(netuid);

    // ---- UIDs and hotkeys ----
    let n = match rng.below(100) {
        0..=1 => 0,
        2..=39 => rng.range(1, 6),
        40..=79 => rng.range(7, 40),
        _ => rng.range(41, u64::from(shape.max_uids.max(41))),
    }
    .min(u64::from(shape.max_uids)) as u16;
    SubnetworkN::<Test>::insert(netuid, n);
    let recent = |rng: &mut Rng| match rng.below(5) {
        0 => 0,
        1 => block,
        2 => block - rng.below(3).min(block),
        // The blocks around the activity cutoff.
        3 => (block - cutoff.min(block) + rng.below(3)).max(1) - 1,
        _ => rng.range(0, block),
    };
    for uid in 0..n {
        Keys::<Test>::insert(netuid, uid, hotkey(uid));
        Uids::<Test>::insert(netuid, hotkey(uid), uid);
        if rng.chance(90) {
            IsNetworkMember::<Test>::insert(hotkey(uid), netuid, true);
        }
        BlockAtRegistration::<Test>::insert(netuid, uid, recent(rng));
    }
    if shape.inconsistent {
        for uid in 0..n {
            if rng.chance(4) {
                // A UID without a hotkey is the state under test.
                #[allow(unknown_lints)]
                Keys::<Test>::remove(netuid, uid);
            } else if rng.chance(3) {
                Keys::<Test>::insert(netuid, uid, hotkey(rng.below(u64::from(n)) as u16));
            }
        }
        for extra in 0..rng.below(3) {
            let uid = n + rng.below(300) as u16;
            let key = if rng.chance(50) {
                U256::from(700_000 + extra)
            } else {
                hotkey(rng.below(u64::from(n.max(1))) as u16)
            };
            Keys::<Test>::insert(netuid, uid, key);
        }
    }

    // ---- Stake ----
    let stake_mode = rng.below(7);
    let rich = rng.below(u64::from(n.max(1))) as u16;
    // The reference sums stake in a signed 64-bit integer part. Keep every sum below it.
    let ceiling = 1_000_000_000_000_000u64;
    for uid in 0..n {
        let alpha = match stake_mode {
            0 => 0,
            1 => {
                if uid == rich {
                    rng.range(1, ceiling)
                } else {
                    0
                }
            }
            2 => 1_000_000,
            3 => rng.below(3),
            4 => rng.range(0, 1_000_000_000_000),
            5 => {
                if rng.chance(20) {
                    rng.range(1, ceiling)
                } else {
                    0
                }
            }
            _ => 1_000_000_000 + u64::from(uid % 3),
        };
        set_stake(netuid, uid, alpha);
        if rng.chance(25) {
            TotalHotkeyAlpha::<Test>::insert(
                hotkey(uid),
                NetUid::ROOT,
                AlphaBalance::from(rng.range(0, 1_000_000_000_000)),
            );
        }
    }
    if n > 0 && rng.chance(35) {
        // Delegation: registered and external parents, and children that take stake away.
        for _ in 0..rng.range(1, 6) {
            let child = hotkey(rng.below(u64::from(n)) as u16);
            let parent = if rng.chance(60) {
                hotkey(rng.below(u64::from(n)) as u16)
            } else {
                let external = U256::from(800_000 + rng.below(4));
                TotalHotkeyAlpha::<Test>::insert(
                    external,
                    netuid,
                    AlphaBalance::from(rng.range(0, 1_000_000_000_000)),
                );
                external
            };
            let proportion = rng.pick(&[1u64, u64::MAX / 3, u64::MAX / 2, u64::MAX]);
            ParentKeys::<Test>::mutate(child, netuid, |parents| parents.push((proportion, parent)));
            ChildKeys::<Test>::mutate(parent, netuid, |children| {
                children.push((proportion, child))
            });
            if rng.chance(15) {
                ChildkeyThresholdSuspended::<Test>::insert(parent, ());
            }
        }
    }
    StakeThreshold::<Test>::put(match rng.below(10) {
        0..=3 => 0,
        4 => 1,
        5 => u64::MAX,
        _ => rng.range(0, 1_000_000_000_000),
    });

    // The validator that the epoch will elect, unless the owner below changes a suspension.
    let (stakes, _, _) = SubtensorModule::get_null_stake_weights_for_network(netuid);
    let validator = SubtensorModule::null_validator_winner(&stakes).unwrap_or(0) as u16;
    SubnetOwnerHotkey::<Test>::insert(
        netuid,
        match rng.below(10) {
            0..=2 => hotkey(validator),
            3..=6 => hotkey(rng.below(u64::from(n.max(1))) as u16),
            _ => U256::from(600_000),
        },
    );
    ValidatorPermit::<Test>::insert(
        netuid,
        (0..rng.pick(&[0u16, n, n, n, n + 2]))
            .map(|_| rng.chance(30))
            .collect::<Vec<_>>(),
    );

    // ---- Mechanisms ----
    let mechanisms = match rng.below(100) {
        0..=27 => 1,
        28..=47 => 2,
        48..=62 => 16,
        63..=96 => rng.range(3, 15),
        _ if shape.inconsistent => rng.pick(&[0u64, 17, 20]),
        _ => 4,
    } as u8;
    MaxMechanismCount::<Test>::put(MechId::from(MAX_MECHANISM_COUNT_PER_SUBNET));
    MechanismCountCurrent::<Test>::insert(netuid, MechId::from(mechanisms));
    match rng.below(20) {
        0..=9 => {}
        10..=17 => MechanismEmissionSplit::<Test>::insert(
            netuid,
            random_split(rng, usize::from(mechanisms.max(1))),
        ),
        // A split of the wrong length: the epoch must split evenly.
        _ => MechanismEmissionSplit::<Test>::insert(
            netuid,
            random_split(rng, usize::from(mechanisms) + 1),
        ),
    }

    let commit_reveal = rng.chance(40);
    SubtensorModule::set_commit_reveal_weights_enabled(netuid, commit_reveal);
    RevealPeriodEpochs::<Test>::insert(netuid, rng.range(1, 3));
    let epoch = rng.range(0, 12);
    SubnetEpochIndex::<Test>::insert(netuid, epoch);
    let some_epoch = |rng: &mut Rng| {
        let latest = epoch + rng.below(3);
        latest - rng.below(6).min(latest)
    };
    let committer = |rng: &mut Rng| match rng.below(10) {
        0..=5 => Keys::<Test>::get(netuid, validator),
        6..=8 => hotkey(rng.below(u64::from(n.max(1))) as u16),
        _ => U256::from(900_000),
    };

    for mechanism in 0..mechanisms.clamp(1, MAX_MECHANISM_COUNT_PER_SUBNET) {
        let index = mechanism_index(netuid, mechanism);
        let len = match rng.below(12) {
            0 if shape.inconsistent => 0,
            1 if shape.inconsistent => n.max(1) - 1,
            2 if shape.inconsistent => n + 2,
            _ => n,
        };
        LastUpdate::<Test>::insert(index, (0..len).map(|_| recent(rng)).collect::<Vec<_>>());

        // The elected validator has a row most of the time. Other UIDs hold rows too.
        for uid in 0..n {
            if (uid == validator && rng.chance(85)) || rng.chance(8) {
                Weights::<Test>::insert(index, uid, random_row(rng, n, uid, shape));
            }
        }
        // Values of an earlier epoch, with any length.
        if rng.chance(50) {
            let stale = |rng: &mut Rng| {
                (0..rng.below(u64::from(n) + 3))
                    .map(|_| PerU16::from_parts(rng.below(65_536) as u16))
                    .collect::<Vec<_>>()
            };
            Incentive::<Test>::insert(index, stale(rng));
            ConsensusByMechanism::<Test>::insert(index, stale(rng));
        }
        // Commits exist whether or not commit-reveal is enabled now.
        if rng.chance(55) {
            for _ in 0..rng.range(1, 3) {
                let queue = (0..rng.range(1, 3))
                    .map(|_| {
                        (
                            H256::repeat_byte(rng.below(250) as u8),
                            some_epoch(rng),
                            recent(rng),
                            0,
                        )
                    })
                    .collect::<VecDeque<_>>();
                WeightCommits::<Test>::insert(index, committer(rng), queue);
            }
        }
        if rng.chance(55) {
            for _ in 0..rng.range(1, 3) {
                let queue = (0..rng.range(1, 3))
                    .map(|_| {
                        (
                            committer(rng),
                            recent(rng),
                            vec![7u8; rng.below(6) as usize].try_into().unwrap(),
                            rng.below(1_000),
                        )
                    })
                    .collect::<VecDeque<_>>();
                TimelockedWeightCommits::<Test>::insert(index, some_epoch(rng), queue);
            }
        }
    }

    // ---- Subnet-level terms of an earlier epoch and voting power ----
    if rng.chance(50) {
        Active::<Test>::insert(netuid, vec![true; rng.below(u64::from(n) + 3) as usize]);
        Emission::<Test>::insert(
            netuid,
            vec![AlphaBalance::from(5u64); rng.below(u64::from(n) + 3) as usize],
        );
    }
    if rng.chance(35) {
        VotingPowerTrackingEnabled::<Test>::insert(netuid, true);
        VotingPowerEmaAlpha::<Test>::insert(
            netuid,
            rng.pick(&[0u64, 1, 100_000_000_000_000_000, 1_000_000_000_000_000_000]),
        );
        match rng.below(4) {
            0 => VotingPowerDisableAtBlock::<Test>::insert(netuid, block - 1),
            1 => VotingPowerDisableAtBlock::<Test>::insert(netuid, block + 5),
            _ => {}
        }
        let mut total = 0u64;
        for _ in 0..rng.below(6) {
            let power = rng.range(0, 1_000_000_000_000);
            let who = committer(rng);
            total = total - VotingPower::<Test>::get(netuid, who) + power;
            VotingPower::<Test>::insert(netuid, who, power);
        }
        TotalVotingPower::<Test>::insert(netuid, total);
    }

    Case {
        netuid,
        uids: n,
        mechanisms,
        budgets: (random_budget(rng), random_budget(rng), random_budget(rng)),
    }
}

/// What the random cases of one test exercised.
#[derive(Debug, Default)]
struct Coverage {
    cases: u32,
    mechanisms: u32,
    /// Mechanisms that paid miners by the validator's weights.
    weighted: u32,
    /// Mechanisms that paid every UID equally.
    fallback: u32,
    /// Subnets with UIDs and no positive stake.
    unstaked: u32,
    multi_mechanism: u32,
    commit_reveal: u32,
    /// Subnets where at least one UID is inactive and one is active.
    mixed_activity: u32,
}

fn run_random_cases(seeds: core::ops::Range<u64>, shape: Shape) -> Coverage {
    let mut coverage = Coverage::default();
    for seed in seeds {
        let mut rng = Rng(seed);
        new_test_ext(1).execute_with(|| {
            let case = random_case(&mut rng, shape);
            println!(
                "seed {seed}: netuid {:?}, {} UIDs, {} mechanisms, budgets {:?}",
                case.netuid, case.uids, case.mechanisms, case.budgets
            );
            let outcome =
                assert_equivalent(case.netuid, case.budgets.0, case.budgets.1, case.budgets.2);
            coverage.cases += 1;
            coverage.multi_mechanism += u32::from(case.mechanisms > 1);
            coverage.unstaked += u32::from(case.uids > 0 && paid(&outcome).1 == 0);
            coverage.commit_reveal += u32::from(
                SubtensorModule::get_commit_reveal_weights_enabled(case.netuid),
            );
            let active = &outcome.storage.active;
            coverage.mixed_activity += u32::from(active.contains(&true) && active.contains(&false));
            for mechanism in 0..case.mechanisms.min(MAX_MECHANISM_COUNT_PER_SUBNET) {
                coverage.mechanisms += 1;
                if paid_by_weights(&outcome, mechanism) {
                    coverage.weighted += 1;
                } else {
                    coverage.fallback += 1;
                }
            }
        });
    }
    println!("{coverage:?}");
    coverage
}

/// Fails when the random cases stop reaching both payout paths.
fn assert_covered(coverage: &Coverage) {
    assert!(coverage.weighted * 5 > coverage.mechanisms, "weighted rows");
    assert!(
        coverage.fallback * 5 > coverage.mechanisms,
        "equal fallback"
    );
    assert!(
        coverage.multi_mechanism * 2 > coverage.cases,
        "mechanism counts"
    );
    assert!(coverage.unstaked * 20 > coverage.cases, "unstaked subnets");
    assert!(coverage.commit_reveal * 4 > coverage.cases, "commit reveal");
    assert!(coverage.mixed_activity * 4 > coverage.cases, "activity");
}

const REGISTERED: Shape = Shape {
    max_uids: 64,
    inconsistent: false,
};
const INCONSISTENT: Shape = Shape {
    max_uids: 64,
    inconsistent: true,
};

#[test]
fn shared_null_epoch_matches_reference_on_random_subnets_a() {
    assert_covered(&run_random_cases(0..1_500, REGISTERED));
}

#[test]
fn shared_null_epoch_matches_reference_on_random_subnets_b() {
    assert_covered(&run_random_cases(1_500..3_000, REGISTERED));
}

#[test]
fn shared_null_epoch_matches_reference_on_random_inconsistent_subnets_a() {
    assert_covered(&run_random_cases(10_000..11_500, INCONSISTENT));
}

#[test]
fn shared_null_epoch_matches_reference_on_random_inconsistent_subnets_b() {
    assert_covered(&run_random_cases(11_500..13_000, INCONSISTENT));
}

#[test]
fn shared_null_epoch_matches_reference_on_random_medium_subnets() {
    let shape = Shape {
        max_uids: 600,
        inconsistent: true,
    };
    run_random_cases(20_000..20_300, shape);
}

#[test]
fn shared_null_epoch_matches_reference_on_random_full_subnets() {
    let shape = Shape {
        max_uids: NULL_UID_BUDGET,
        inconsistent: true,
    };
    run_random_cases(30_000..30_040, shape);
}
