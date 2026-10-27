use super::*;
use frame_support::traits::Get;
use frame_system::pallet_prelude::BlockNumberFor;
use pallet_alpha_assets::AlphaAssetsInterface;
use scale_info::prelude::string::String;
use sp_runtime::traits::Zero;
use subtensor_runtime_common::{AlphaBalance, NetUid};

pub(crate) const MIGRATION_NAME: &[u8] = b"migrate_backfill_historical_alpha_burned";

/// `(netuid, generation_registered_at, historical_burned_alpha)` reconstructed at
/// mainnet block 8,283,784, immediately before `AlphaBurned` tracking began.
///
/// Before that deployment, explicit alpha burns and miner incentives withheld from
/// owner-associated UIDs removed claims without recording a counter. The amounts are
/// the conservation remainder:
///
/// `SubnetAlphaOut - TotalHotkeyAlpha - all pending alpha claims`
///
/// plus the matching `SubnetAlphaOut` correction applied earlier in this upgrade.
/// The reconstruction scanned blocks 4,920,351 through 8,283,784 to verify every burn
/// path and was checked against the new counter after its deployment. Only generations
/// still live at scan block 8,780,303 are embedded; the registration-block guard also
/// prevents applying a row if its netuid is recycled before this migration executes.
pub(crate) const HISTORICAL_ALPHA_BURNED: &[(u16, u64, u64)] = &[
    (1, 1_497_824, 661_707_044_125_477),
    (2, 2_734_060, 448_755_318_641_353),
    (3, 4_165_565, 578_474_866_822_790),
    (4, 1_411_451, 539_567_236_063_726),
    (5, 2_491_604, 1_278_433_691_163_126),
    (6, 3_219_949, 161_892_134_501_551),
    (7, 2_627_691, 36_084_978_602_376),
    (8, 1_477_264, 609_870_070_120_154),
    (9, 1_489_797, 944_559_342_501_061),
    (10, 2_869_647, 495_108_691_195_807),
    (11, 2_918_568, 1_087_091_829_288_036),
    (12, 2_256_433, 1_299_624_537_206_613),
    (13, 1_907_637, 444_081_197_134_941),
    (14, 4_848_444, 960_182_196_930_445),
    (15, 7_366_897, 301_749_291_195_706),
    (17, 2_840_556, 305_407_983_298_724),
    (18, 1_604_679, 245_065_757_998_180),
    (19, 1_956_072, 801_324_778_843_975),
    (20, 1_970_929, 1_094_173_011_650_198),
    (21, 3_156_578, 650_713_208_209_082),
    (22, 2_009_702, 1_019_276_438_426_431),
    (23, 2_063_528, 967_233_605_317_608),
    (24, 2_538_424, 505_685_726_352_258),
    (25, 2_998_801, 648_845_379_719_658),
    (26, 8_123_781, 17_606_271_531_596),
    (27, 1_727_132, 1_001_642_315_084_892),
    (28, 4_878_363, 1_203_330_815_035_081),
    (29, 3_379_782, 610_829_352_749_762),
    (30, 3_250_216, 486_388_020_387_918),
    (31, 7_173_591, 437_000_151_331_724),
    (32, 2_515_294, 17_419_272_873_098),
    (33, 2_943_950, 494_543_923_300_457),
    (34, 3_493_948, 359_469_273_754_254),
    (35, 3_037_158, 266_886_343_998_939),
    (36, 7_894_898, 121_987_667_067_834),
    (37, 3_212_175, 985_435_365_945_143),
    (38, 7_284_230, 345_810_821_435_472),
    (39, 3_280_104, 1_287_420_117_604_175),
    (41, 3_394_182, 769_681_094_468_115),
    (42, 3_613_591, 206_796_478_056_192),
    (43, 3_408_582, 678_317_244_908_562),
    (44, 3_550_319, 423_455_578_235_596),
    (45, 3_633_154, 834_928_101_300_642),
    (46, 3_919_107, 780_421_061_400_320),
    (47, 7_340_355, 366_716_413_649_807),
    (48, 3_856_677, 695_492_461_656_953),
    (49, 6_783_158, 573_769_022_103_814),
    (50, 4_763_204, 383_336_277_470_646),
    (51, 3_966_206, 1_065_224_008_905_985),
    (52, 3_989_825, 853_315_981_760_243),
    (53, 4_203_869, 355_141_382_485_346),
    (54, 4_742_549, 458_319_381_232_759),
    (55, 4_703_386, 306_585_844_101_300),
    (56, 4_312_927, 627_232_573_084_813),
    (57, 8_057_320, 93_750_241_023_900),
    (59, 4_401_833, 369_047_970_107_529),
    (60, 4_796_992, 507_498_715_394_405),
    (61, 4_457_976, 694_963_647_168_562),
    (62, 4_474_225, 183_144_981_369_889),
    (63, 4_885_578, 725_439_396_179_942),
    (64, 4_531_295, 223_211_670_756_295),
    (65, 4_950_813, 735_211_819_505_774),
    (66, 4_958_013, 817_890_833_634_185),
    (67, 7_236_936, 181_452_933_284_774),
    (68, 4_972_413, 629_386_445_545_985),
    (69, 8_138_182, 57_832_819_758_834),
    (70, 7_787_562, 132_569_838_883_663),
    (71, 5_048_438, 779_562_258_823_670),
    (72, 5_064_327, 161_494_057_515_857),
    (73, 5_160_047, 1_245_107_989_914_506),
    (74, 5_086_205, 531_791_422_968_235),
    (75, 5_102_795, 581_181_547_138_717),
    (76, 7_574_784, 296_127_405_249_152),
    (77, 5_128_460, 140_886_896_551_370),
    (78, 7_966_145, 58_070_016_871_684),
    (79, 5_173_967, 380_435_716_169_563),
    (80, 7_151_800, 47_626_242_449_288),
    (81, 5_203_057, 830_779_100_179_480),
    (82, 8_026_517, 50_318_415_917_529),
    (83, 5_231_190, 345_172_824_104_195),
    (84, 8_085_297, 39_094_022_971_873),
    (85, 5_258_781, 395_120_544_109_912),
    (87, 7_208_725, 423_704_834_120_304),
    (88, 5_299_805, 132_836_998_007_560),
    (89, 5_313_364, 416_002_458_400_820),
    (91, 7_633_645, 238_851_859_703_207),
    (93, 5_370_681, 903_913_959_207_004),
    (94, 6_962_737, 463_616_359_168_619),
    (95, 5_403_674, 752_120_660_373_289),
    (96, 7_692_872, 93_094_905_548_345),
    (97, 7_735_450, 46_566_942_010_408),
    (98, 5_445_992, 1_079_336_385_082_409),
    (100, 6_693_448, 458_140_544_083_257),
    (101, 5_481_350, 709_791_313_021_521),
    (102, 7_840_965, 67_558_146_161_703),
    (104, 5_528_520, 340_321_172_598_343),
    (105, 6_841_399, 427_306_744_552_955),
    (106, 5_558_480, 694_428_357_403_395),
    (107, 7_457_580, 314_712_961_241_019),
    (108, 7_105_263, 397_895_877_171_947),
    (109, 7_257_480, 403_221_764_503_845),
    (110, 5_606_062, 653_602_202_903_777),
    (111, 5_615_562, 494_642_104_691_108),
    (112, 5_633_022, 752_495_769_344_929),
    (113, 7_119_664, 459_198_730_858_941),
    (114, 7_312_241, 319_277_386_475_235),
    (115, 5_683_635, 660_512_253_515_748),
    (117, 5_710_859, 289_424_602_021_989),
    (118, 5_724_794, 164_626_458_446_094),
    (119, 5_740_660, 649_075_554_396_218),
    (120, 5_749_344, 170_312_526_711_417),
    (121, 5_766_528, 1_023_076_320_965_598),
    (122, 8_238_082, 22_766_972_908_103),
    (123, 5_794_330, 149_823_723_964_164),
    (124, 5_813_454, 804_691_305_771_930),
    (125, 5_834_408, 987_462_933_759_858),
    (126, 7_525_773, 288_254_403_748_941),
    (127, 5_848_837, 950_782_986_715_338),
    (128, 5_856_038, 186_014_794_503_201),
];

/// Adds the pre-tracking mainnet burn remainder to `AlphaAssets::AlphaBurned`.
pub fn migrate_backfill_historical_alpha_burned<T: Config>() -> Weight {
    let migration_name = MIGRATION_NAME.to_vec();
    let mut weight = T::DbWeight::get().reads(1);

    if HasMigrationRun::<T>::get(&migration_name) {
        log::info!(
            "Migration '{}' already executed - skipping",
            String::from_utf8_lossy(MIGRATION_NAME)
        );
        return weight;
    }

    let genesis_hash = frame_system::Pallet::<T>::block_hash(BlockNumberFor::<T>::zero());
    weight.saturating_accrue(T::DbWeight::get().reads(1));
    let mainnet_genesis =
        hex_literal::hex!("2f0555cc76fc2840a25a6ea3b9637146806f1f44b090c175ffde2a7e5ab36c03");
    let mut corrected = 0u64;

    if genesis_hash.as_ref() == mainnet_genesis {
        for &(netuid, expected_registered_at, historical_burned) in HISTORICAL_ALPHA_BURNED {
            let netuid = NetUid::from(netuid);
            let registered_at = NetworkRegisteredAt::<T>::get(netuid);
            weight.saturating_accrue(T::DbWeight::get().reads(1));

            if registered_at != expected_registered_at {
                log::error!(
                    "Skipping historical alpha burn for netuid {:?}: registered_at={}, expected={}",
                    netuid,
                    registered_at,
                    expected_registered_at,
                );
                continue;
            }

            let _ = T::AlphaAssets::burn_alpha(netuid, AlphaBalance::from(historical_burned));
            weight.saturating_accrue(T::DbWeight::get().reads_writes(1, 1));
            corrected = corrected.saturating_add(1);
        }
    } else {
        log::info!(
            "Migration '{}' skipped outside mainnet",
            String::from_utf8_lossy(MIGRATION_NAME)
        );
    }

    HasMigrationRun::<T>::insert(&migration_name, true);
    weight.saturating_accrue(T::DbWeight::get().writes(1));

    log::info!(
        "Migration '{}' completed with {} historical alpha burn corrections",
        String::from_utf8_lossy(MIGRATION_NAME),
        corrected,
    );
    weight
}
