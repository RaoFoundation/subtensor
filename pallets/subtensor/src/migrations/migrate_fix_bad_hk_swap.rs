use super::*;
use frame_support::{traits::Get, weights::Weight};
use frame_system::pallet_prelude::BlockNumberFor;
use log;
use safe_math::FixedExt;
use scale_info::prelude::string::String;
use sp_core::crypto::Ss58Codec;
use sp_runtime::AccountId32;
use substrate_fixed::{traits::ToFixed, types::U64F64};

pub fn decode_account_id32<T: Config>(ss58_string: &str) -> Option<T::AccountId> {
    let account_id32: AccountId32 = AccountId32::from_ss58check(ss58_string).ok()?;
    let mut account_id32_slice: &[u8] = account_id32.as_ref();
    T::AccountId::decode(&mut account_id32_slice).ok()
}

pub fn try_restore_shares<T: Config>() -> Weight {
    let mut weight = T::DbWeight::get().reads(1);

    let effected_hotkey = "5HK5tp6t2S59DywmHRWPBVJeJ86T61KjurYqeooqj8sREpeN";
    let hk_as_acctid: T::AccountId =
        if let Some(hk_as_acctid) = decode_account_id32::<T>(effected_hotkey) {
            hk_as_acctid
        } else {
            log::error!("Failed to decode hotkey: {}", effected_hotkey);
            return weight;
        };

    let effected_netuid = 59.into();

    #[rustfmt::skip]
	let diffs: [(&str, i64); 112] = [
		("5Fn9SqQhx5bhDua7AGgkKxxk3gfZ75WWBGCMPeKH1WBgPaMQ", -2375685930981_i64),
		("5Fnhtm7cpxEbZaChnRZ8yWoF8MXVxmobkmLRehh5bkYtyZA9", -4090996138227),
		("5C7j3w2zz1SVejRuFrb2zFWHXT7UfG7eWA87KXL1WyV5KLVR", -607494031),
		("5DthZ1rvnXBb9oXVNtrMaMsDAnRxBPZCjD6fdRdeqC3fg1ca", -17022477949),
		("5F7BkPL3EVjKTYMbBkEmPAtTZQSGeyNzFPaf1DtebPFmJsJ7", -4016510),
		("5EefisctzgWdVGFQaL4LjFFacTE7dM4YJVNy3ogGBQoapTU1", -13106893093),
		("5CwkvpBxHCaRK9xBC2n6WdhpF5zg9t5WLkGorASaoErdynFQ", 439139249152),
		("5FU7ErUtmi22xuqeeCYVpNZp6WVSSL98hqDi5iyeZbkXtkbe", -35958768555),
		("5D7HL8T95qkHQTPFjgSFCjRoeM7oE3vQBYjiR1kAPbPxcMKu", -201914811997),
		("5HL3pPdDFY94Qdf8VnbfT4W6LXFkpd68Y5GSGzNJfntMdGZX", -235660917467),
		("5EcYAz8SBKWsogA6meJmVXcwVp4tjCvw3ZnJE6UXTyWNUdF2", -500070769668),
		("5EoE3c7XMf8TN3yudAaFjv4yvjtWYRviHcXi73EXkLHmWTCB", -86442928436),
		("5CMDjL7t2biHGREBwrmd8renD74FLEhjCVqfJG2MXckWBwDu", 1039317),
		("5CVGKimL4cLgyTqvYKQbPKYFZfiztsdczU7HrwNdSFKbbn5D", 4224201),
		("5HmZnEcW4eHbXmUEFWJbc4GHnBBYEK8ZPsFa25PuEmP5iuwM", -13156128),
		("5FNa4J4fTKh555CEyXHgR29RicSm8nTEHx36utTa4MJepJyX", -9519954),
		("5Gun93uQgffYpxqMKSmfG18AHiQW7Z2GR2dfPPR8W188vJYc", -1127662),
		("5HW1C4js4RyjQqNwALSUZC8NJ2WinD5Si2X2XkstXrMW2uYo", -34457336758),
		("5EqMhjdLY9h64ui2mizRZyBp1mEPJ7s4TsfAxQSQkAFmMfzE", -9346443829744),
		("5H3XwzydgE2XUGoJCR4dSj7tkd7uxZDJqik69hux2DBcruom", -1215347774),
		("5DjkmYpCUX6dBTvGoyN9j4QZhtMPhdcywDE8cJ8Qq1vg4X6e", -3603984447),
		("5E7Z7Btjz74XpZLH5fRzfZqiHCo4j9PXKfqi88kQ5MFrds34", -823907380854),
		("5DSBWN4hN9413C6o6A2hR9tYUbHjWsQqPRV74GrnCrMkCGJx", -309708781),
		("5FMLRmKPqsTsMbakpVUwoYro1P64QXVNTWyzNDugaNwSKRzF", -137525398263),
		("5EFZf5pnTqLegv6gxCrb6TKBQBGz9xLJNK8x9eR273cSons6", -1521760918),
		("5CGAGEuMLaidBDk8bDZKJb23dxRSP1wLenLALGLw8BTG1E3W", -544739696),
		("5HSzRtcQjD5KP6Nh2GVSS16aLDe6q9R33Wpu6s2eEeeo3AYS", -2309184790),
		("5DALvFDcfANQJcWz6AXMfDqabnoZhdDMoH6FxqYUibug1ja7", -369405632507),
		("5Fy8iWkpcbsskmEN1nYZDdS9zKh167Em9RRYisoss7jaYXxi", 15257429),
		("5CQCVTRyqJgZKBDmtHzpoF8su6BScLcNGbX8t3WMm5qYbbJH", -10721968),
		("5DXZByh2NS4MU61a1aaLrcLYpyzpJgHe95TEBdcEN2cF1SA5", -655946136),
		("5EX5yAYiABFzKDQJDe1kRVwFm3XRRY4HyLMe4Vu9A5U2VEVT", -325581360246),
		("5FvabwjtyW887gtc7vUnUc47KVhy17UeaNLRjzTg5nkVACMP", -77588524213),
		("5HTbYi5cmgWJxvyTy9JeYdtnjoDzjXnEXTGFsPEVx9iRPmVF", -53542953784),
		("5CWzmvA17MAMQ9mnAecLxFXS2N8846rz6T7m4QNHyVtJVq4j", 2672295922502),
		("5DSYntgHZY4krYUtkkQZyyoffVtu5e8rYWhXuhs832zY6YKy", -2680205688),
		("5EYyTFyLDqXscaa5VtXTvUc3x2ow2TeT8G12ZDMZwE6uFWPQ", -39165843935),
		("5CohfM1qdyNwdeJEex1Zyht3S2WS48rV993DmVbyKs2mEEd6", -4004685632),
		("5Gx6Y7UQD39Latgxigr6mHbnh1herpwNPau2PjvzwLWEjXL3", -559504),
		("5Hh4Efq5WDwe8URjjUqUNX8KxtMwLHLViwoRvXfEpXQCZakh", -32541090531),
		("5GWRHC7Nd8njqTPsdJkp6ngniCCBu9UjGhLfxp2jF1fPrfZ4", -5394093031),
		("5GNAB64UN32krzr3Xxu5LW6naeu2P3XULcdBCR9VZ5Libyit", -24884230),
		("5EEz25th1nYNM5xR1UsyFFAUaXMjdHqLxZ3wUjyHokYbXHku", -12525171),
		("5HKJq4JCS9xoKdYhcRnsRp1bodovba7ncd5KTYVwfReKaxHT", -408133990236),
		("5DXs7x664RL5NdSW77DTseLiu84unstuHGuqvmY61UtJwzRN", -3095078614148),
		("5CDfdDaA2p9sK1ia5yMVYfzgtFs2e1TrSAxuQqXoS28Lcrxf", -1032856892),
		("5Ecg4vD2zKXHDFhQqogWq1dZdijPsDty8rGsZu3raeoJSiXb", -995678),
		("5C5Yg63TNLb68Tu819qXd3Bt4giG8mAPzLmAFSqa2HC1R5Rm", -40818739830910),
		("5HHH25Wuf9rmVuk9cMKU1hCCPJ1qbHBd1SyHj91R3fMT36yb", -391416057906),
		("5GKGGE5YLHoDciYJ6Ec2YnUP3SykSQPA47hqmwBP63EtVrd9", -413944553000),
		("5CSi9ZLyiXfLeYtEFaZSBuTofNMRnXEJEJE9CS4gGaT6CkWt", -17811605275),
		("5CSoA7QVdFHHBZz53bbRV2mC5vhL64ehhWa8ibtLppmt2n3J", -65701320107),
		("5GpA5BtfMMX52rXztrha79YqfwR4YaSfTuAcb48Yt73U4h71", -2194562),
		("5Euz5wpb4xiDWfV1A6AKK6i6ca3WoZQD5hCVyf1fws8GXh4z", -6143407839874),
		("5DZzmhCG7SMK3LwrkmHZ8ZBwaAByMjfBpEid14nNQdxHipCE", -386645),
		("5Fc9Vo3hkbr6bPxJpjQo5sQ43L5Hc2G8R5BdqRYF8psvB5pw", 55668553),
		("5GuSHC3iowySHLDW4pEyEZE6PKxKP62YpJYJyBy5tijzAnYz", -159317636526),
		("5HVVZrUBPvjYHiwaSvtvaN9GZogoznM49m2AEmVW6RXnYCka", -1995572213),
		("5EcGpeV2wjkCVsBjsBifSWbdcqH98b6oEY8beDY59c4fXkhw", -177096614584),
		("5GnCjvWJEESwVNFZzy85zbBzw26etuEt87WiqsE3ee2Ws1wm", -1961445),
		("5GWuPUpTuChAqKxvU22TRLvRkBFiyWWZnq9cLpJN6SSvkho1", -94157569391),
		("5FXHf7q5rvBXnzQgmsa31Db9rjcRy6ZHKMiyDSb8Vs5p2msN", -688433531658),
		("5GbxkzytnvbRuNQ7qxPpfPuWMoeitS8V4KDY9jSshE5fDegD", -19085313),
		("5Gus1B7c9uWkky7Yawh2tKR1V6AMh5DbqUBPq881JHqeqVqY", -16101671818),
		("5DLhRdbvWkYYScDmwx4QgJfieSN4apBWbZ2yno3MfgbR8hBP", -21062025),
		("5Cg5kVyNEs7MWWRHU8X5MHwX5cN3aegvC4RBt2JK19w2GiR8", -2593737050),
		("5Dkushsxtc8AdCf287MtTYHQv9DoZeBRpttUpBtmyFhGy3uR", -48672832345630),
		("5EqNqVsHj9bQVyEujcm62zjMYUFhTLY7rTP854txSrJzyoco", -3828526),
		("5Dea6d6nKErEbRQ4MBGuCALn8NZ2xo4kaa51hB5KMriPBkEM", -1560192853875),
		("5DNt2XDWdeMd4H92FLnfUvkqyXzmavezHvzLboP3VgT1xLZV", -831964576998),
		("5FKtFoTeK8aaG6HZTrDgvoYHVQ5NY4S9VyV7W5K74cWcwLYA", -60823501166),
		("5GEBanZKUU7Hrf8K2VNi33HxyJRstgQ3WD3odHgvMj2nPbhi", -98946626902),
		("5CUtw7LYB2n2bzgXt6YnmKDHt6PsB3kKAyD9azYJNcRG8TNg", -9779588557490),
		("5EynbF72b12fbgMvEeL1vJSY342rCryNbuwxFivU1Xevtmv3", -17314385200455),
		("5CapiZRuULed8ConS1gbjMVgnwcT5JnQah7tx6sZnK7sJJuJ", -5810972),
		("5DnaxLaNduf41WM6WWZ4fkzcGzWNWx6eLJyQpSaMueUGCsaU", -12668760),
		("5Cqz9SChYPxTFZ2623rE2aQQ5ttQoLwZ8yfwYgiZyQDANqZn", -683549),
		("5C8ZcLzF23GrXKdH4Pg3ZXC3vKQsF5PM8VvhzzxzTQksgj8e", -44720570590),
		("5GuNsmoswrP6hTKZkKcpTpZftTMKrmnCHvTL2V3NHJy2fpen", -5042891812715),
		("5F1TYDkLnP36HHY5btigxyKUPzBraxdrU1aX1bqFfPfcfnzU", -1189104279832),
		("5Dc384z9HuTGF6oratZs1fLciCHtPZaLhrHfCVw82a5AikWZ", -616163196988),
		("5DhcaEUsRKhZQ31qRffJqjtLmFkbVaCebn8nVjYhvB4KJtX5", -17746006723),
		("5HYE7z3xTcrN1rqz54NyZRAkehFfRMcaEcdoMq5g5ATET5wQ", 212509751245),
		("5F97DdEVTy9gPCtN6jkJJENDJuQiRGiwbMVSL74qRq8FCq5W", 2225287736222),
		("5FUVN133rSvuKXgsXKMR2ZEaysxZjkRUFUWS1UMyNGre9xFV", -73216740161),
		("5CZeimtfpRqQgPxVwr1MzfG2Sok8E1AMERHo6vUmEdRS5JiU", -3937802),
		("5Eqq2JwGh7qbtnjPiFEPmmnHxs3S4J4Ahg8fr4sybZV1tPdY", -173406860562),
		("5ERfDw6K3GmQqwqsEG6foFtu7VsYGifPi556UJKQsBnfbHKN", 96022588728),
		("5Ek8RkU6KMv5Fx7yivRVoQkuJYAKhULWiLWDpbGG4hvR9HFD", 968139369093),
		("5HpCpGALzqgnDTP1HXFiuhzD5MFaDTRHjXBCvaMY9LNNRkT9", 104943979521),
		("5FYqS77gxW9gHG8id1YYPS7Cd4TQmNUMhF8h3S77Fq2VvvRQ", 729199757977),
		("5FtBqMg13pNf1N6TwfG6BmwyaDM77mkeQ16UTHsGasrVDedX", 131457064336),
		("5GbnWR2XhWrRMt123SdrLbR9G2a4N5dtzA3TSu3Czkzoeu7x", 2295599153),
		("5GgiowcCG4kLpwkCTGxxQJQv8WwKFyBQ6McPRmqKtWPy8EaK", 113838605389),
		("5FL5YtYozpUAGaiVWonpbwEYdEMij3obJHSH3ACY4vgWmDgy", 8689039),
		("5Egq58bxRv7boM2s3rnDxx1udnkzxPQ23HuoqohVxjh9RenC", 216373234348),
		("5FRGeeEgRNR8U33FDKvN7yUgts8zR3qRJH4yKKWoR9GswBRb", 2196574958718),
		("5FnhSy79BPYyrmmFsbckinQw1fLiLqqPkQL2vgZwPxbRfu3k", 42319631507),
		("5Hj8jMhqAv7cfyRh5STfbZefMhv17QxZ1RxWq9jNcLAEsRRo", 132216702183491),
		("5EXYTGMqumAH6RLQgHwkMEMnSvHcpHc89R6U8krfNJTYWm9J", 504320264499),
		("5EFh8ctzmytXURqrCTUBWHTs87f7TMWB6XKUzdqxKXVUtvS2", 2209599669432),
		("5CqVqEcRBkw7Gm2reJ33cj7puR9W2Tq7qsLxSruV1BgnMqKN", 1033387458788),
		("5D79enmLSGimsruoraGagofhaSeYJZvGUqFCCrr83ZfZs1HS", 7591184215233),
		("5HbpyjsvyXLWtf1QT1CyNUdyut6scM5dM7ytm8hoxFvRtU1i", 129833188275),
		("5CnxCi7CdEriWSdw4LcXdbtjodxA6uTat4gBm4wuT9QToMdo", 3132978),
		("5G48fiQjhAd8hc4rYc6GituCuAPKznL28jyyyq1auMyZiG4t", 514913328178),
		("5FFGjW2hJ7tQ41qghSsLP4cVmA8j9pZVSrr2CrLG7fQAsLHJ", 346794972723),
		("5FWjnxeRMtMFxRc9kvZKCG5iJAyyz2kmXV8u3kqyiXizZtiz", 225939835005),
		("5CUw3sB4oxd3dVSHUr3kxsB591VEjaPzr444KkfjwVFnLRfJ", 208250614494),
		("5EaBhxNUwMRyKsaeA2BEjDCrvwE5J8FDSpfCHK9gGmnmbhCa", 278083207003),
		("5GHJ5HxFxYQyVoNFUxR3JCqqCKRumaFCY7N5zMxwF4CpRUWr", 1381466224829),
		("5H1WgA7ET3FmEarJK6qc1vaTWbNd6g41mgvyLRkysrH4MDdo", 774889),
	];

    let mut total_burned: AlphaBalance = 0.into();
    let mut total_given: U64F64 = U64F64::from_num(0);
    let mut total_lost: U64F64 = U64F64::from_num(0);

    for (ck, diff) in diffs {
        if let Some(ck_as_acctid) = decode_account_id32::<T>(ck) {
            let curr_bal = U64F64::from_num(
                Pallet::<T>::get_stake_for_hotkey_and_coldkey_on_subnet(
                    &hk_as_acctid,
                    &ck_as_acctid,
                    effected_netuid,
                )
                .to_u64(),
            );
            weight = weight.saturating_add(T::DbWeight::get().reads(3));

            if diff > 0 {
                log::debug!(
                    target: "undohkswap",
                    "Coldkey '{}' has balance {:?} diff: {}",
                    ck,
                    curr_bal,
                    diff
                );

                // remove excess, if possible
                let diff_fixed = U64F64::from_num(diff);
                total_given = total_given.saturating_add(diff_fixed);

                let able_to_remove = diff_fixed.min(curr_bal);
                log::debug!(
                    target: "undohkswap",
                    "Coldkey '{}' able to remove {:?} of {:?}",
                    ck,
                    able_to_remove,
                    curr_bal
                );

                let as_alpha_balance: AlphaBalance =
                    able_to_remove.saturating_to_num::<u64>().into();
                Pallet::<T>::decrease_stake_for_hotkey_and_coldkey_on_subnet(
                    &hk_as_acctid,
                    &ck_as_acctid,
                    effected_netuid,
                    as_alpha_balance,
                );
                weight = weight.saturating_add(T::DbWeight::get().reads_writes(3, 3));
                total_burned = total_burned.saturating_add(as_alpha_balance);
            } else {
                total_lost = total_lost.saturating_add(U64F64::from_num(diff.abs()));
            }
        } else {
            log::error!(
                target: "undohkswap",
                "Coldkey '{}' failed to decode",
                ck
            )
        }
    }

    let not_burned = total_given.saturating_sub(total_burned.to_fixed());
    if not_burned > 0 {
        log::warn!(
            target: "undohkswap",
            "Did not burn {:?} Alpha. Burned {:?}",
            not_burned,
            total_burned,
        )
    }

    // value that can be returned per unit lost
    let value_per_lost: U64F64 = U64F64::from_num(total_burned).safe_div(total_lost);
    let mut total_returned: AlphaBalance = 0.into();
    for (ck, diff) in diffs {
        if let Some(ck_as_acctid) = decode_account_id32::<T>(ck) {
            if diff < 0 {
                // lose some, return as much as we can, proportionally
                let diff_fixed = U64F64::from_num(diff.abs());
                // Calculate
                let diff_prop = diff_fixed.saturating_mul(value_per_lost);

                let as_alpha_balance: AlphaBalance = diff_prop.saturating_to_num::<u64>().into();
                Pallet::<T>::increase_stake_for_hotkey_and_coldkey_on_subnet(
                    &hk_as_acctid,
                    &ck_as_acctid,
                    effected_netuid,
                    as_alpha_balance,
                );
                weight = weight.saturating_add(T::DbWeight::get().reads_writes(3, 3));
                total_returned = total_returned.saturating_add(as_alpha_balance);
            }
        } else {
            log::error!(
                target: "undohkswap",
                "Coldkey '{}' failed to decode",
                ck
            )
        }
    }

    let not_returned = total_lost.saturating_sub(total_returned.to_fixed());
    if not_returned > 0 {
        log::warn!(
            target: "undohkswap",
            "Did not return {:?} of {:?} Alpha. Returned {:?}",
            not_returned,
            total_lost,
            total_returned,
        )
    }

    weight
}

pub fn migrate_fix_bad_hk_swap<T: Config>() -> Weight {
    let migration_name = b"migrate_fix_bad_hk_swap".to_vec();
    let mut weight = T::DbWeight::get().reads(1);

    // Skip if already executed
    if HasMigrationRun::<T>::get(&migration_name) {
        log::info!(
            target: "runtime",
            "Migration '{}' already run - skipping.",
            String::from_utf8_lossy(&migration_name)
        );
        return weight;
    }

    // Only run on mainnet.
    // Mainnet genesis: 0x2f0555cc76fc2840a25a6ea3b9637146806f1f44b090c175ffde2a7e5ab36c03
    let genesis_hash = frame_system::Pallet::<T>::block_hash(BlockNumberFor::<T>::zero());
    weight = weight.saturating_add(T::DbWeight::get().reads(1));
    let genesis_bytes = genesis_hash.as_ref();
    let mainnet_genesis =
        hex_literal::hex!("2f0555cc76fc2840a25a6ea3b9637146806f1f44b090c175ffde2a7e5ab36c03");
    if genesis_bytes == mainnet_genesis {
        weight = weight.saturating_add(try_restore_shares::<T>());
    }

    // Mark migration done
    HasMigrationRun::<T>::insert(&migration_name, true);
    weight = weight.saturating_add(T::DbWeight::get().writes(1));

    log::info!(
        target: "runtime",
        "Migration '{}' completed.",
        String::from_utf8_lossy(&migration_name)
    );

    weight
}
