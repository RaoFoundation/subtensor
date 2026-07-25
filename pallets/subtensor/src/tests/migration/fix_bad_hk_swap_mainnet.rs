#![allow(
    unused,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used
)]
//! Bad hotkey-swap repair — mainnet cases.

use super::helpers::*;
use super::prelude::*;

#[test]
fn test_migrate_fix_bad_hk_swap_mainnet() {
    new_test_ext(1).execute_with(|| {
        use crate::migrations::migrate_fix_bad_hk_swap::*;

        let netuid = NetUid::from(59);
        // Add subnet 59
        add_network(netuid, 10, 0);
        SubtokenEnabled::<Test>::insert(netuid, true);
        SubnetMechanism::<Test>::insert(netuid, 1);

        let hotkey = "5HK5tp6t2S59DywmHRWPBVJeJ86T61KjurYqeooqj8sREpeN";
        let hotkey_account_id = decode_account_id32::<Test>(hotkey).expect("Invalid hotkey");

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

        for (coldkey, diff) in diffs {
            let coldkey_account_id = decode_account_id32::<Test>(coldkey).expect("Invalid coldkey");
            if diff > 0 {
                SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
                    &hotkey_account_id,
                    &coldkey_account_id,
                    netuid,
                    diff.unsigned_abs().into(),
                );
            }
        }

        let w = try_restore_shares::<Test>();
        assert!(!w.is_zero(), "weight must be non-zero");

        // Check stake is near 0 for all positive entires and near diff for all negative
        for (coldkey, diff) in diffs {
            let coldkey_account_id = decode_account_id32::<Test>(coldkey).expect("Invalid coldkey");

            let stake_float: f64 = num_traits::ToPrimitive::to_f64(
                &SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                    &hotkey_account_id,
                    &coldkey_account_id,
                    netuid,
                )
                .to_u64(),
            )
            .expect("float conv fail");
            if diff > 0 {
                log::debug!("diff: {} for ck: {}", diff, coldkey);
                assert_relative_eq!(stake_float, 0_f64, max_relative = 0.001_f64);
            } else {
                let diff_float: f64 =
                    num_traits::ToPrimitive::to_f64(&diff.unsigned_abs()).expect("float conv fail");
                assert_relative_eq!(stake_float, diff_float, max_relative = 0.001_f64);
            }
        }
    });
}

#[test]
fn test_migrate_fix_bad_hk_swap_mainnet_some_exits() {
    // test with some of the gainers have exited fully or partially before the migration
    // i.e. balance is less than they owe
    new_test_ext(1).execute_with(|| {
        use crate::migrations::migrate_fix_bad_hk_swap::*;

        let netuid = NetUid::from(59);
        // Add subnet 59
        add_network(netuid, 10, 0);
        SubtokenEnabled::<Test>::insert(netuid, true);
        SubnetMechanism::<Test>::insert(netuid, 1);

        let hotkey = "5HK5tp6t2S59DywmHRWPBVJeJ86T61KjurYqeooqj8sREpeN";
        let hotkey_account_id = decode_account_id32::<Test>(hotkey).expect("Invalid hotkey");

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

        for (coldkey, diff) in diffs {
            let coldkey_account_id = decode_account_id32::<Test>(coldkey).expect("Invalid coldkey");
            if diff > 0 {
                SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
                    &hotkey_account_id,
                    &coldkey_account_id,
                    netuid,
                    diff.unsigned_abs().into(),
                );
            }
        }

        // For one of the gainers, remove some of the stake
        let idx = 6;
        let gained_ck = diffs[idx].0;
        let coldkey_account_id = decode_account_id32::<Test>(gained_ck).expect("Invalid coldkey");
        SubtensorModule::decrease_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_account_id,
            &coldkey_account_id,
            netuid,
            num_traits::ToPrimitive::to_u64(
                &(num_traits::ToPrimitive::to_f64(&diffs[idx].1).expect("float conv fail")
                    * 0.9_f64)
                    .abs(),
            )
            .expect("u64 conv fail")
            .into(),
        );

        let w = try_restore_shares::<Test>();
        assert!(!w.is_zero(), "weight must be non-zero");

        // Check stake is near 0 for all positive entires except the one we removed
        // Check the stake for all negative entries is proportional to the amount they lost
        let total_lost: f64 = diffs
            .iter()
            .map(|(_, diff)| {
                if diff.is_negative() {
                    num_traits::ToPrimitive::to_f64(&diff.saturating_abs())
                        .expect("float conv fail")
                } else {
                    0_f64
                }
            })
            .sum::<f64>();
        let mut total_returned = 0_f64;
        for (coldkey, diff) in diffs {
            let coldkey_account_id = decode_account_id32::<Test>(coldkey).expect("Invalid coldkey");

            let stake_float: f64 = num_traits::ToPrimitive::to_f64(
                &SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                    &hotkey_account_id,
                    &coldkey_account_id,
                    netuid,
                )
                .to_u64(),
            )
            .expect("float conv fail");
            if diff > 0 {
                log::debug!("diff: {} for ck: {}", diff, coldkey);
                assert_relative_eq!(stake_float, 0_f64, max_relative = 0.001_f64);
            } else {
                total_returned += stake_float;
            }
        }

        for (coldkey, diff) in diffs {
            let coldkey_account_id = decode_account_id32::<Test>(coldkey).expect("Invalid coldkey");
            let stake_float: f64 = num_traits::ToPrimitive::to_f64(
                &SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                    &hotkey_account_id,
                    &coldkey_account_id,
                    netuid,
                )
                .to_u64(),
            )
            .expect("float conv fail");
            if diff < 0 {
                // Should get a return proportional to the amount they lost
                // versus the amount that was able to be recovered
                let prop_returned: f64 = num_traits::ToPrimitive::to_f64(&diff.abs())
                    .expect("float conv fail")
                    / total_lost
                    * total_returned;
                assert_relative_eq!(stake_float, prop_returned, max_relative = 0.001_f64);
            }
        }
    });
}

#[test]
fn test_migrate_fix_bad_hk_swap_mainnet_has_more() {
    // test with some of the gainers have a balance higher than the gain before the migration
    new_test_ext(1).execute_with(|| {
        use crate::migrations::migrate_fix_bad_hk_swap::*;

        let netuid = NetUid::from(59);
        // Add subnet 59
        add_network(netuid, 10, 0);
        SubtokenEnabled::<Test>::insert(netuid, true);
        SubnetMechanism::<Test>::insert(netuid, 1);

        let hotkey = "5HK5tp6t2S59DywmHRWPBVJeJ86T61KjurYqeooqj8sREpeN";
        let hotkey_account_id = decode_account_id32::<Test>(hotkey).expect("Invalid hotkey");

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

        for (coldkey, diff) in diffs {
            let coldkey_account_id = decode_account_id32::<Test>(coldkey).expect("Invalid coldkey");
            if diff > 0 {
                SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
                    &hotkey_account_id,
                    &coldkey_account_id,
                    netuid,
                    diff.unsigned_abs().into(),
                );
            }
        }

        // For one of the gainers, add some extra stake
        let idx = 6;
        let gained_ck = diffs[idx].0;
        let coldkey_account_id = decode_account_id32::<Test>(gained_ck).expect("Invalid coldkey");
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_account_id,
            &coldkey_account_id,
            netuid,
            num_traits::ToPrimitive::to_u64(
                &((num_traits::ToPrimitive::to_f64(&diffs[idx].1).expect("float conv fail")
                    * 0.9_f64)
                    .abs()),
            )
            .expect("u64 conv fail")
            .into(),
        );
        let extra_balance = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_account_id,
            &coldkey_account_id,
            netuid,
        )
        .saturating_sub(
            num_traits::ToPrimitive::to_u64(&diffs[idx].1.abs())
                .expect("float conv fail")
                .into(),
        );
        assert!(
            extra_balance.to_u64() > 0_u64,
            "extra balance must be positive"
        );

        let w = try_restore_shares::<Test>();
        assert!(!w.is_zero(), "weight must be non-zero");

        // Check stake is near 0 for all positive entires except the one we removed
        // Check the stake for all negative entries is proportional to the amount they lost
        let total_lost: f64 = diffs
            .iter()
            .map(|(_, diff)| {
                if diff.is_negative() {
                    num_traits::ToPrimitive::to_f64(&diff.saturating_abs())
                        .expect("float conv fail")
                } else {
                    0_f64
                }
            })
            .sum::<f64>();
        let mut total_returned = 0_f64;
        for (coldkey, diff) in diffs {
            let coldkey_account_id = decode_account_id32::<Test>(coldkey).expect("Invalid coldkey");

            let stake_float: f64 = num_traits::ToPrimitive::to_f64(
                &SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                    &hotkey_account_id,
                    &coldkey_account_id,
                    netuid,
                )
                .to_u64(),
            )
            .expect("float conv fail");
            if diff > 0 {
                if coldkey == gained_ck {
                    // this CK should retain the extra balance
                    assert_relative_eq!(
                        stake_float,
                        num_traits::ToPrimitive::to_f64(&extra_balance.to_u64())
                            .expect("float conv fail"),
                        max_relative = 0.001_f64
                    );
                } else {
                    assert_relative_eq!(stake_float, 0_f64, max_relative = 0.001_f64);
                }
            } else {
                total_returned += stake_float;
            }
        }

        for (coldkey, diff) in diffs {
            let coldkey_account_id = decode_account_id32::<Test>(coldkey).expect("Invalid coldkey");
            let stake_float: f64 = num_traits::ToPrimitive::to_f64(
                &SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                    &hotkey_account_id,
                    &coldkey_account_id,
                    netuid,
                )
                .to_u64(),
            )
            .expect("float conv fail");
            if diff < 0 {
                // Should get a return proportional to the amount they lost
                // versus the amount that was able to be recovered
                let prop_returned: f64 = num_traits::ToPrimitive::to_f64(&diff.abs())
                    .expect("float conv fail")
                    / total_lost
                    * total_returned;
                assert_relative_eq!(stake_float, prop_returned, max_relative = 0.001_f64);
            }
        }
    });
}

#[test]
fn test_migrate_fix_bad_hk_swap_mainnet_some_entries() {
    // test with some of the losers have an existing balance before the migration
    new_test_ext(1).execute_with(|| {
        use crate::migrations::migrate_fix_bad_hk_swap::*;

        let netuid = NetUid::from(59);
        // Add subnet 59
        add_network(netuid, 10, 0);
        SubtokenEnabled::<Test>::insert(netuid, true);
        SubnetMechanism::<Test>::insert(netuid, 1);

        let hotkey = "5HK5tp6t2S59DywmHRWPBVJeJ86T61KjurYqeooqj8sREpeN";
        let hotkey_account_id = decode_account_id32::<Test>(hotkey).expect("Invalid hotkey");

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

        for (coldkey, diff) in diffs {
            let coldkey_account_id = decode_account_id32::<Test>(coldkey).expect("Invalid coldkey");
            if diff > 0 {
                SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
                    &hotkey_account_id,
                    &coldkey_account_id,
                    netuid,
                    diff.unsigned_abs().into(),
                );
            }
        }

        // For one of the losers, add some extra stake
        let idx = 0;
        let lost_ck = diffs[idx].0;
        let coldkey_account_id = decode_account_id32::<Test>(lost_ck).expect("Invalid coldkey");
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_account_id,
            &coldkey_account_id,
            netuid,
            num_traits::ToPrimitive::to_u64(
                &((num_traits::ToPrimitive::to_f64(&diffs[idx].1).expect("float conv fail")
                    * 0.9_f64)
                    .abs()),
            )
            .expect("u64 conv fail")
            .into(),
        );
        let extra_balance = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_account_id,
            &coldkey_account_id,
            netuid,
        );
        assert!(!extra_balance.is_zero(), "extra balance must be non-zero");

        let w = try_restore_shares::<Test>();
        assert!(!w.is_zero(), "weight must be non-zero");

        // Check stake is near 0 for all positive entires except the one we removed
        // Check the stake for all negative entries is proportional to the amount they lost
        let total_lost: f64 = diffs
            .iter()
            .map(|(_, diff)| {
                if diff.is_negative() {
                    num_traits::ToPrimitive::to_f64(&diff.saturating_abs())
                        .expect("float conv fail")
                } else {
                    0_f64
                }
            })
            .sum::<f64>();
        let mut total_returned = 0_f64;
        for (coldkey, diff) in diffs {
            let coldkey_account_id = decode_account_id32::<Test>(coldkey).expect("Invalid coldkey");

            let stake_float: f64 = num_traits::ToPrimitive::to_f64(
                &SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                    &hotkey_account_id,
                    &coldkey_account_id,
                    netuid,
                )
                .to_u64(),
            )
            .expect("float conv fail");
            if diff > 0 {
                assert_relative_eq!(stake_float, 0_f64, max_relative = 0.001_f64);
            } else if coldkey == lost_ck {
                total_returned += stake_float
                    - num_traits::ToPrimitive::to_f64(&extra_balance.to_u64())
                        .expect("float conv fail");
            } else {
                total_returned += stake_float;
            }
        }

        for (coldkey, diff) in diffs {
            let coldkey_account_id = decode_account_id32::<Test>(coldkey).expect("Invalid coldkey");
            let stake_float: f64 = num_traits::ToPrimitive::to_f64(
                &SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                    &hotkey_account_id,
                    &coldkey_account_id,
                    netuid,
                )
                .to_u64(),
            )
            .expect("float conv fail");
            if diff < 0 {
                // Should get a return proportional to the amount they lost
                // versus the amount that was able to be recovered
                let prop_returned: f64 = num_traits::ToPrimitive::to_f64(&diff.abs())
                    .expect("float conv fail")
                    / total_lost
                    * total_returned;

                let mut expected_stake: f64 = prop_returned;
                if coldkey == lost_ck {
                    // this CK should retain the extra balance
                    expected_stake = prop_returned
                        + num_traits::ToPrimitive::to_f64(&extra_balance.to_u64())
                            .expect("float conv fail");
                }

                assert_relative_eq!(stake_float, expected_stake, max_relative = 0.001_f64);
            }
        }
    });
}
