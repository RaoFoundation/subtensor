#![allow(clippy::unwrap_used)]

use super::mock::*;
use crate::*;
use frame_support::weights::{Weight, WeightMeter};
use subtensor_runtime_common::NetUid;

type TestEntry = (NetUid, u64);

fn db_read() -> Weight {
    <Test as frame_system::Config>::DbWeight::get().reads(1)
}

fn db_writes(n: u64) -> Weight {
    <Test as frame_system::Config>::DbWeight::get().writes(n)
}

fn run_cleanup(
    weight_meter: &mut WeightMeter,
    entries: Vec<TestEntry>,
    target: NetUid,
    writes_per_match: u64,
) -> (bool, Option<TestEntry>, sp_std::vec::Vec<u64>) {
    let removed = sp_std::cell::RefCell::new(sp_std::vec::Vec::new());
    let (read_all, last_item) = SubtensorModule::remove_storage_entries_for_netuid(
        weight_meter,
        entries.into_iter(),
        |&(netuid, _)| netuid == target,
        |(_, id)| id,
        |id| removed.borrow_mut().push(*id),
        writes_per_match,
    );
    (read_all, last_item, removed.into_inner())
}

#[test]
fn remove_storage_entries_for_netuid_empty_iterator() {
    new_test_ext(0).execute_with(|| {
        let limit = Weight::from_parts(u64::MAX, u64::MAX);
        let mut weight_meter = WeightMeter::with_limit(limit);

        let (read_all, last_item, removed) =
            run_cleanup(&mut weight_meter, vec![], NetUid::from(1), 1);

        assert!(read_all);
        assert!(last_item.is_none());
        assert!(removed.is_empty());
        assert_eq!(weight_meter.consumed(), Weight::zero());
    });
}

#[test]
fn remove_storage_entries_for_netuid_removes_matching_entries() {
    new_test_ext(0).execute_with(|| {
        let target = NetUid::from(1);
        let entries = vec![
            (NetUid::from(1), 10),
            (NetUid::from(2), 20),
            (NetUid::from(1), 30),
        ];
        let mut weight_meter = WeightMeter::with_limit(Weight::from_parts(u64::MAX, u64::MAX));

        let (read_all, last_item, removed) = run_cleanup(&mut weight_meter, entries, target, 1);

        assert!(read_all);
        assert_eq!(last_item, Some((NetUid::from(1), 30)));
        assert_eq!(removed, vec![10, 30]);

        let expected = db_read()
            .saturating_mul(3)
            .saturating_add(db_writes(1).saturating_mul(2));
        assert_eq!(weight_meter.consumed(), expected);
    });
}

#[test]
fn remove_storage_entries_for_netuid_skips_non_matching_entries() {
    new_test_ext(0).execute_with(|| {
        let target = NetUid::from(99);
        let entries = vec![
            (NetUid::from(1), 10),
            (NetUid::from(2), 20),
            (NetUid::from(3), 30),
        ];
        let mut weight_meter = WeightMeter::with_limit(Weight::from_parts(u64::MAX, u64::MAX));

        let (read_all, last_item, removed) = run_cleanup(&mut weight_meter, entries, target, 1);

        assert!(read_all);
        assert_eq!(last_item, Some((NetUid::from(3), 30)));
        assert!(removed.is_empty());
        assert_eq!(weight_meter.consumed(), db_read().saturating_mul(3));
    });
}

#[test]
fn remove_storage_entries_for_netuid_stops_when_read_budget_exhausted() {
    new_test_ext(0).execute_with(|| {
        let target = NetUid::from(1);
        let entries = vec![
            (NetUid::from(2), 10),
            (NetUid::from(2), 20),
            (NetUid::from(1), 30),
        ];
        // Budget for two reads only; the third entry is never scanned.
        let limit = db_read().saturating_mul(2);
        let mut weight_meter = WeightMeter::with_limit(limit);

        let (read_all, last_item, removed) = run_cleanup(&mut weight_meter, entries, target, 1);

        assert!(!read_all);
        assert_eq!(last_item, Some((NetUid::from(2), 20)));
        assert!(removed.is_empty());
        assert_eq!(weight_meter.consumed(), limit);
    });
}

#[test]
fn remove_storage_entries_for_netuid_stops_when_write_budget_exhausted() {
    new_test_ext(0).execute_with(|| {
        let target = NetUid::from(1);
        let entries = vec![(NetUid::from(1), 10), (NetUid::from(1), 20)];
        // Two reads and one write: first match is removed, second match reads but cannot write.
        let limit = db_read().saturating_mul(2).saturating_add(db_writes(1));
        let mut weight_meter = WeightMeter::with_limit(limit);

        let (read_all, last_item, removed) = run_cleanup(&mut weight_meter, entries, target, 1);

        assert!(!read_all);
        assert_eq!(last_item, Some((NetUid::from(1), 10)));
        assert_eq!(removed, vec![10]);
        assert_eq!(weight_meter.consumed(), limit);
    });
}

#[test]
fn remove_storage_entries_for_netuid_respects_writes_per_match() {
    new_test_ext(0).execute_with(|| {
        let target = NetUid::from(1);
        let entries = vec![(NetUid::from(1), 10), (NetUid::from(1), 20)];
        let writes_per_match = 2_u64;
        // Two reads and two writes: first match is removed, second match reads but cannot write.
        let limit = db_read()
            .saturating_mul(2)
            .saturating_add(db_writes(writes_per_match));
        let mut weight_meter = WeightMeter::with_limit(limit);

        let (read_all, last_item, removed) =
            run_cleanup(&mut weight_meter, entries, target, writes_per_match);

        assert!(!read_all);
        assert_eq!(last_item, Some((NetUid::from(1), 10)));
        assert_eq!(removed, vec![10]);
        assert_eq!(weight_meter.consumed(), limit);
    });
}

#[test]
fn remove_storage_entries_for_netuid_defers_removals_until_after_scan() {
    new_test_ext(0).execute_with(|| {
        use sp_std::{cell::Cell, rc::Rc};

        let target = NetUid::from(1);
        let entries = vec![
            (NetUid::from(1), 10),
            (NetUid::from(1), 20),
            (NetUid::from(1), 30),
        ];
        let scan_finished = Rc::new(Cell::new(false));
        let mut weight_meter = WeightMeter::with_limit(Weight::from_parts(u64::MAX, u64::MAX));

        struct ScanTrackingIter {
            items: sp_std::vec::Vec<TestEntry>,
            index: usize,
            scan_finished: Rc<Cell<bool>>,
        }

        impl Iterator for ScanTrackingIter {
            type Item = TestEntry;

            fn next(&mut self) -> Option<Self::Item> {
                if self.index >= self.items.len() {
                    self.scan_finished.set(true);
                    return None;
                }
                let item = self.items.get(self.index).copied()?;
                self.index += 1;
                Some(item)
            }
        }

        let scan_finished_for_ops = Rc::clone(&scan_finished);
        let (read_all, last_item) = SubtensorModule::remove_storage_entries_for_netuid(
            &mut weight_meter,
            ScanTrackingIter {
                items: entries,
                index: 0,
                scan_finished,
            },
            |&(netuid, _)| netuid == target,
            |(_, id)| id,
            |_| {
                assert!(
                    scan_finished_for_ops.get(),
                    "removal ops must run only after the iterator is exhausted"
                )
            },
            1,
        );

        assert!(read_all);
        assert_eq!(last_item, Some((NetUid::from(1), 30)));
    });
}
