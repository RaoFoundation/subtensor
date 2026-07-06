use super::*;
use crate::{Config, HasMigrationRun, SubnetLocked, TaoBalance};
use frame_support::weights::Weight;
use log;
use scale_info::prelude::string::String;
use subtensor_runtime_common::NetUid;

pub fn migrate_restore_subnet_locked<T: Config>() -> Weight {
    // Track whether we've already run this migration
    let migration_name = b"migrate_restore_subnet_locked".to_vec();
    let mut weight = T::DbWeight::get().reads(1);

    // Snapshot: NetworkLastLockCost at (registration_block + 1) for each netuid.
    const SUBNET_LOCKED: &[(u16, u64)] = &[
        (65, 37_274_536_408),
        (66, 65_230_444_016),
        (67, 114_153_284_032),
        (68, 199_768_252_064),
        (69, 349_594_445_728),
        (70, 349_412_366_216),
        (71, 213_408_488_702),
        (72, 191_341_473_067),
        (73, 246_711_333_592),
        (74, 291_874_466_228),
        (75, 247_485_227_056),
        (76, 291_241_991_316),
        (77, 303_154_601_714),
        (78, 287_407_417_932),
        (79, 254_935_051_664),
        (80, 255_413_055_349),
        (81, 249_790_431_509),
        (82, 261_343_249_180),
        (83, 261_361_408_796),
        (84, 201_938_003_214),
        (85, 264_805_234_604),
        (86, 223_171_973_880),
        (87, 180_397_358_280),
        (88, 270_596_039_760),
        (89, 286_399_608_951),
        (90, 267_684_201_301),
        (91, 284_637_542_762),
        (92, 288_373_410_868),
        (93, 290_836_604_849),
        (94, 270_861_792_144),
        (95, 210_595_055_304),
        (96, 315_263_727_200),
        (97, 158_244_884_792),
        (98, 168_102_223_900),
        (99, 252_153_339_800),
        (100, 378_230_014_000),
        (101, 205_977_765_866),
        (102, 149_434_017_849),
        (103, 135_476_471_008),
        (104, 147_970_415_680),
        (105, 122_003_668_139),
        (106, 133_585_556_570),
        (107, 200_137_144_216),
        (108, 106_767_623_816),
        (109, 124_280_483_748),
        (110, 186_420_726_696),
        (111, 249_855_564_892),
        (112, 196_761_272_984),
        (113, 147_120_048_727),
        (114, 84_021_895_534),
        (115, 98_002_215_656),
        (116, 89_944_262_256),
        (117, 107_183_582_952),
        (118, 110_644_724_664),
        (119, 99_380_483_902),
        (120, 138_829_019_156),
        (121, 111_988_743_976),
        (122, 130_264_686_152),
        (123, 118_034_291_488),
        (124, 79_312_501_676),
        (125, 43_214_310_704),
        (126, 64_755_449_962),
        (127, 97_101_698_382),
        (128, 145_645_807_991),
    ];

    if HasMigrationRun::<T>::get(&migration_name) {
        log::info!(
            target: "runtime",
            "Migration '{}' already run - skipping.",
            String::from_utf8_lossy(&migration_name)
        );
        return weight;
    }

    let mut inserted: u32 = 0;
    let mut total_rao: u128 = 0;

    // ── 1) Re-insert the historical values ────────────────────────────────
    for &(netuid_u16, amount_rao_u64) in SUBNET_LOCKED.iter() {
        let key: NetUid = NetUid::from(netuid_u16);
        let amount: TaoBalance = amount_rao_u64.into();

        SubnetLocked::<T>::insert(key, amount);

        inserted = inserted.saturating_add(1);
        total_rao = total_rao.saturating_add(amount_rao_u64 as u128);

        weight = weight.saturating_add(T::DbWeight::get().writes(1));
    }

    // ── 2) Mark migration done ────────────────────────────────────────────
    HasMigrationRun::<T>::insert(&migration_name, true);
    weight = weight.saturating_add(T::DbWeight::get().writes(1));

    log::info!(
        target: "runtime",
        "Migration '{}' completed - inserted {} SubnetLocked entries; total≈{} RAO.",
        String::from_utf8_lossy(&migration_name),
        inserted,
        total_rao
    );

    weight
}
