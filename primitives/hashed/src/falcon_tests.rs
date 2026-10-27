#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use super::*;

fn header(profile: u8) -> ([u8; 32], Header) {
    for candidate in 0u8..=255 {
        let seed = [candidate; 32];
        let public_key = public_key_from_seed(profile, &seed).unwrap();
        let current = commitment(profile, &public_key).unwrap();
        let descriptor = Descriptor {
            version: VERSION,
            algorithm: profile,
            initial_commitment: current,
        };
        let account = account_id(&descriptor);
        if is_fixed_profile(profile) && !rejects_legacy_public_keys(&account) {
            continue;
        }
        return (
            seed,
            Header {
                account,
                version: VERSION,
                algorithm: profile,
                generation: 0,
                public_key,
                next_commitment: if is_fixed_profile(profile) {
                    current
                } else {
                    [0x47; 64]
                },
            },
        );
    }
    panic!("bounded fixture derivation failed")
}

#[test]
fn all_profiles_bind_entire_transaction_and_preserve_raw_wire_lengths() {
    let payload = vec![0x42; 1024];
    for profile in [
        ALGORITHM_512,
        FIXED_ALGORITHM_512,
        ALGORITHM_1024,
        FIXED_ALGORITHM_1024,
    ] {
        let (seed, header) = header(profile);
        let proof = header.sign(&seed, &payload, &[0x59; 32]).unwrap();
        assert!(proof.verify(&payload));
        assert_eq!(proof.encode().len(), proof_len(profile).unwrap());
        assert_eq!(header.encode().len(), header_len(profile).unwrap());
        assert_eq!(SignerAddress::from_bytes(&proof.encode()).unwrap(), proof);
        let mut changed = payload.clone();
        changed[999] ^= 1;
        assert!(!proof.verify(&changed));
        for field in 0..6 {
            let mut bad = proof.clone();
            match field {
                0 => bad.account[0] ^= 1,
                1 => bad.generation = 1,
                2 => bad.next_commitment[0] ^= 1,
                3 => bad.public_key[42] ^= 1,
                4 => bad.signature[50] ^= 1,
                _ => {
                    bad.algorithm = if profile % 2 == 0 {
                        profile + 1
                    } else {
                        profile - 1
                    }
                }
            }
            assert!(!bad.verify(&payload), "profile {profile}, field {field}");
        }
        let bytes = proof.encode();
        for size in [0, 33, 42, bytes.len() - 1] {
            assert!(SignerAddress::from_bytes(&bytes[..size]).is_err());
        }
        let mut extended = bytes;
        extended.push(0);
        assert!(SignerAddress::from_bytes(&extended).is_err());
    }
}

#[test]
fn fixed_accounts_are_self_authenticating_and_rotating_profiles_are_not() {
    for profile in [FIXED_ALGORITHM_512, FIXED_ALGORITHM_1024] {
        let (_, mut header) = header(profile);
        assert!(header.is_valid_stateless());
        assert!(rejects_legacy_public_keys(&header.account));
        header.generation = 1;
        assert!(!header.is_valid_stateless());
        header.generation = 0;
        header.next_commitment[0] ^= 1;
        assert!(!header.is_valid_stateless());
    }
    for profile in [ALGORITHM_512, ALGORITHM_1024] {
        assert!(!header(profile).1.is_valid_stateless());
    }
}

#[test]
fn detached_messages_bind_context_profile_and_canonical_padding() {
    for profile in [ALGORITHM_512, ALGORITHM_1024] {
        let seed = [0x39; 32];
        let key = public_key_from_seed(profile, &seed).unwrap();
        let signature = sign_message(profile, &seed, b"message", b"purpose", &[0x51; 32]).unwrap();
        assert!(verify_message(
            profile, &key, b"message", b"purpose", &signature
        ));
        assert!(!verify_message(
            profile, &key, b"other", b"purpose", &signature
        ));
        assert!(!verify_message(
            profile,
            &key,
            b"message",
            b"different",
            &signature
        ));
        assert!(!verify_message(
            profile + 1,
            &key,
            b"message",
            b"purpose",
            &signature
        ));
        let mut wrong_header = signature.clone();
        wrong_header[0] ^= 1;
        assert!(!verify_message(
            profile,
            &key,
            b"message",
            b"purpose",
            &wrong_header
        ));
        let mut bad_padding = signature.clone();
        *bad_padding.last_mut().unwrap() = 0xff;
        assert!(!verify_message(
            profile,
            &key,
            b"message",
            b"purpose",
            &bad_padding
        ));
        assert!(!verify_message(
            profile,
            &key,
            b"message",
            b"purpose",
            &signature[..signature.len() - 1]
        ));
    }
}

#[test]
fn payload_bound_is_enforced_without_encoding_a_large_allocation() {
    struct Oversized;
    impl Encode for Oversized {
        fn encode_to<O: Output + ?Sized>(&self, output: &mut O) {
            for _ in 0..=MAX_PAYLOAD_LEN / 1024 {
                output.write(&[0; 1024]);
            }
        }
    }
    let (seed, header) = header(ALGORITHM_512);
    let proof = header.sign(&seed, b"payload", &[1; 32]).unwrap();
    assert!(!proof.verify_encoded(&Oversized));
}

#[test]
fn registration_descriptor_preserves_old_tags_and_routes_falcon() {
    for profile in [ALGORITHM_512, ALGORITHM_1024] {
        let descriptor = Descriptor {
            version: VERSION,
            algorithm: profile,
            initial_commitment: [1; 64],
        };
        assert!(descriptor.is_supported());
        assert!(descriptor.is_rotating());
        assert_eq!(
            crate::RegistrationDescriptor::from(descriptor).encode()[0],
            2
        );
        assert_eq!(
            Descriptor::from_bytes(&descriptor.encode()).unwrap(),
            descriptor
        );
    }
    let old = Descriptor {
        version: VERSION,
        algorithm: crate::mldsa::ALGORITHM,
        initial_commitment: [1; 64],
    };
    assert_eq!(crate::RegistrationDescriptor::from(old).encode()[0], 1);
}

// Original Falcon KATs distributed by Thomas Pornin's fn-dsa-vrfy 0.3.0
// under the Unlicense. These fixtures predate this wallet implementation.
#[test]
fn original_falcon_known_answer_vectors() {
    for (public, message, signature) in [ORIGINAL_512, ORIGINAL_1024] {
        let public = hex::decode(public).unwrap();
        let message = hex::decode(message).unwrap();
        let mut signature = hex::decode(signature).unwrap();
        let key = VerifyingKeyStandard::decode(&public).unwrap();
        assert!(key.verify(&signature, &DOMAIN_NONE, &HASH_ID_ORIGINAL_FALCON, &message));
        signature[50] ^= 1;
        assert!(!key.verify(&signature, &DOMAIN_NONE, &HASH_ID_ORIGINAL_FALCON, &message));
    }
}

const ORIGINAL_512: (&str, &str, &str) = (
    concat!(
        "091164798255c8c721e8aa5a4ae3ab9ad824d9f98a81724d26517cd6d2354ceb51b30a006d40ab3bb8a43f4b50b74000",
        "d9b098d0a903291c9eb172b994e1e7afac4d2c952944715e086a1980047d3822c75187d74fa2184a6a1f9fe30eac998b",
        "00aa8689be52925d69672ddd4b17918f827c156d9458a0b05f1c9ac007e56c86f5e84ab846df2b8449e8a99154be3024",
        "74e747dc868052f7e146b03eb00e09f39dc28bc4810793245a6ab04dc6974957ace9a0b8ec994a9a19994b3a14bc5c61",
        "2b884fe028069d65f776da0eb6a28504b7836e964600b9c3ef730562ddc87be93b9186aadab807f615557ac4911fcb00",
        "301612e690845109e456e70a99bfbdf8da07625f9fea908339d85ca995766835c4d99ffa8708adaa936c319fc7cfc648",
        "2e940f452e2bdf8c8ca147178e587139cde26b562c4e71b425bae6c7abb20f134cee7e8cd5019589dddb6caaf025d2a4",
        "cf28859b52fb20aa44782b86cf4a2f716c1d2963509c29ba3d65a18eaa6b29baa93f2b8f0dd063722f284fa969da0a4c",
        "dfc98c0abfe60c8e938429da80496fb49dc43f6a40646eac71f6b98f5de2960e4a02bba6d13db9fb445b59ddbef0fbb9",
        "e09e08ad3917b4a78f9869a625a705c77f1bce6ad973d09e590d42b09503fa9220f17820a0fea7053e798a02d0047ebb",
        "b866cbd65c68c78b1721de87acd566dda80e3c5ed7b108e79e6620afd2f1589302b63e62aae2c826ac7332f0acfab756",
        "c688d8926d58f4242517c98e917d62aadd57ae51fa3c70d1d19bbeb03f6857c1bb0aeed7674b5bea84fa67150a1fd214",
        "66cb9a5a13acb1329130d25ff778f6619619eca65e794d8b3518979688c8b219d4496d867a53fde4659125a3a194c714",
        "b5f5b80684cebc95a8a3059e08ab2d04fc3b10cb938f1e53021ea40d0a1e229bdf08a105b59ea6c32308260644abe756",
        "720a71fee11c280b0ca02a947ca22295dd17e4652dc6587e374bd05b9538f06433209aa21bca958bcec2fe9f75682190",
        "a17660da7207ec2fd42ed8240842dccd0551c7207d1ce851ac49f07689e2ae7de379b4133497482cf7846df70e586b71",
        "0f44bb7f263c19151af89ad1ad6152d6478c6d3f65fed4d0ad1823b74491e0b56a00644ec178363c13a509208f4f8c83",
        "e4d5eebf7e3cbb193018b257798c82f86dab852e4900aa41091d72f10101cec640281fe3b1618402893d9607aeba2044",
        "ed42a843b88d32e680fc5b849ae193d4a00cc524993d561b8e112441c933225005",
    ),
    "dbd71151d7a9b83b0b6e2556ac824225",
    concat!(
        "39d33b744e2ceadc9c9deb583e2d091f65e623e6dfeafbb27b287137d08ace8fae59e59b76568913b72f6a8fce049c15",
        "4bc6a5494ffe4a5b6d8d75fdec7395e47bded536b270314dd4073d72a536ed7f9e96fa48f04c427b3ad34396078df9cf",
        "3271fce47194ccefa1071a7ef24098372aad45a6e2929c830f8aa3256869134778290ac8e5a56583a1348cc5a5997a93",
        "7583a6d02408ce8ad695bcf6a48cb97b943c088d10ec1886312a0e39df86883429fbebe62eb49ca2956143ce53063dc4",
        "7129d33c2ab5dbdb5df0f1514606e71beafce39a5b0aaa437e2c2efeac7b71be4bfafdeedc3752876a6b8eeba5d4c65e",
        "24ceeb5b978855d79cd30c61adfe6cf31c8bff9b56a631cd635a83af5a8c3a294990b1ef79d4c64f9f004f0c429fb9f3",
        "551b79aef1b4a4d41b54a7c92332ccfac30e894f99ead77a98ce93ae0d2def8c65508a7c957c6b4dae61a5284b4ce8e2",
        "b65b4c7d9e4a5f7aef0492b78ed76a33d20c32db58902276b51b3cd56bb07534a88d4025515aaad33f45548fecea106b",
        "5bea66b4a0fbb08c13abe08adef96a04d68f729bbd0d3539623829791d47a0b873b9bee6dc78475d069cee90e6fd63ee",
        "3cecdc473bd1323735fde745ad754171eb30d614daee543fbfbb59bb89a05908173d0f7a48ab6f8872b30a2f996ba6eb",
        "556f950d749d5269f0b3356ae4b44daf8e3667594e41b794cb481d11cc8be2ca06b90e9094cb1c03448164ddd71e0b94",
        "8a6f5015211e46bb12a1216412e33ed8c690c637bd33581c6a0cd593c12f79c2a320dd71e203c3184ca08fcfcb0d52e9",
        "6b4d9adec31d2f153a1b76d834ce00b9cc9437d91e74382bc7bd0e609aadbd9775d0eb15af5575c461a55ba52ed12523",
        "649f9ff922a37923c99f49cea601c9496f9c0e7e33e77085a7fcd839f789a00000000000000000000000",
    ),
);

const ORIGINAL_1024: (&str, &str, &str) = (
    concat!(
        "0aab5887ca451b1e5cf5464503dca9b5f44c3a7786a9b1648014f224b4a1164840584034104513f614af92440417c008",
        "6b513a5514d01ccfb4b53d7c583dc90cbe16586821652b79d049c56bacb67339e3253f60a2597c4f4920e2c9bfe00924",
        "02517a7e54e6d4bba8d681908ca39cecbc445ab976b4d6c34a76ad68ff086110497bd72fcede519542b387959955f24e",
        "4a75642f0c5617b2b7a9c8bd3e064be560120d0d2e376e6b869ac2e8c8925f591718c71b0a1d7a90e61a429a995da258",
        "2cf8470a7e554ef8c7eed71f2a8e8114bc2b755aa67a62560c23712819943b2301223d3f5b6a056a41a02e24b29dc50d",
        "95560601dc1a6bf15184c52fd2ba70e431a91a4b9de44c61c6ce0db8a19ad164d5a1b11df302f19fe773bc39565c2f67",
        "362af543b6de0302cab89bd40d271711841ba74b9b96059bd6989e905a309e64678b8b2cfcf81adf9cfa31e247456301",
        "a37145412956c09a40e1dfd8d7cac123b5bc0791aaa8131126c887c186790923abeac61abd780065158b2eb562ec30a4",
        "9255b7fcfa9a8ca15879f95938354608b8da33686fc7498140743003124183a2ca55d3dad7b000b4c7cb2f10055a572a",
        "92ac9d1cc6eaf721afef1a56859087a4677dd9b76822624e4e9de4a02780d89a5534c8f1840a482233a95befb5342c55",
        "c0f02c8a3dd6fd370fa17485e65f932fee0f50c8b8e78a525f1ce61f11e7d40a03fd6eb99f97c8b262a92598c8d497b6",
        "2ed6e36618033cbb9992ac554834d36b4a2fda56cc4d2615df402aa2afc3da8a1dad86a9c6e6d99695a6d10167c17500",
        "d8df74a4ca841cc9347a330ffbb0a90bc00dcf9b2310fba40752ea2df4c0b862a025678ee9296bd8e060ca7a8385907e",
        "3a2a4bf2962ee71e7ddcd7d3d0e708b94bca1ea0cc6380abd52e8b5c0378b6ea6a55c3646532d5b740740c20a5180180",
        "a156c19aa48715d9214e2129c049489a80dec9e157066a2844227f0de3788e68c13f19d1042eeba9210fc847c8b5b823",
        "29c678963d94e129b14d4a9de9fac7224552cc66a3dc85bf99cc7389cab90636be851c5bae91acd5a53d118dbee23209",
        "164c1e7daddc282008cc5c021dd8cad2810aa24ee3769d8c070d8f13445ac2bb0eba1bdd2d5d76ba114666d8a76295d0",
        "c1c9d96e98e49679cea5151b9e7472781d67a9b4b238c1e6ceb7f26a13086a8d66da7daa7bae6c6972b0fb82a5f55919",
        "cebb8a5ce346d9f863e7d49f75d9a7355e29a00de4395a462758ba9207da5f21430b56fd3121967f67346456f8e098a2",
        "bab3872921205406f427c7134243aaab9847d59eacca83350e20248274b3b69152ac9b9806b60e4b904a64a5609629f0",
        "2d7289973d5790bf4cdae4f599d4bfc25e95f3e93ca70d8952cd6b776de8ee90e1847562c81eb6dccefb0881678386c0",
        "7c084cc6daaa88a840ed45daf0a79036240fd47aaf09bb6849b2011cdb51dc4649f81e2010fe353659d0a58f41bc05a7",
        "a93db7c48851c42caa41356110d4d86c0f10ec135116d125c4e863e2cb8c5be5ef1a2449d287ae34e693ee1c82ecf4c5",
        "58252d79fef5548ee09e0d799f2511a8edbcb699e4bac17238c6173013e3619d35b6eba4654419f92503c7ae8d7fedfb",
        "08bc91a5a3ac2c04da04e89081ffe2cf9aa892bcb05ad3102846f9197b2455080796ddf23d474213cc05f90a425018a0",
        "547749c3697b5905e6b50bb28b7a4fd3bc57a76f9ef4d71f2b208b507e7a47856b4c08f6c198a26e681aefd3ef2ece42",
        "1275f3652a529cc4df987e54761d1d9352edcd3b1dd11e93dc61b35f999e51e68e1c86daf4433d4cb898b41ad595d690",
        "2034b2d2bcd7b7a825d0593b8d0c9f8eec2901293f67b9f9e8149dd15f550f044f9c52870078640f19218cdacc932d9f",
        "7026b9a0860d0f0e47f69fe4b1dd875f6504907fef746d0d0122c381bd60c14c50c8e4b72b1de7f6886f358d5c90c425",
        "4cb7293e29aabfd22b311648252185b216ddfd007eebdfaa35ce0ad619256ba964f18599915fc03d2510631855657210",
        "da5c04138597a290aa1d701ef7a51c67af8ac8f566ded7a04c0eb482ff9ecf4f3259d5e9ea2154ae437bb52b7f2cea69",
        "329b5c4111690c7347d77441f8e6f4f9de24486ae6d9de9f2b41764fda9c832c8699ddb92d9cfc9365e3b45759889b72",
        "d8026b69df2bd5bf57d2498713f6467bd207616f796004b0a4ada981acc98f44c9b0bef8474187444ae1fabb920dfea4",
        "85ba6123199b7162fc64128e8539f263a1350878bcace361a5d65f61e8ca54f4da1526bd66529f8624065625c70003f0",
        "6b3a0e44801b35b4ca036110a31a868a45980c085c23241750c98f2a20d01a07e820618689b58615458b5b35c1843c0a",
        "b2465d1ab2d446cb981e19593c42c1923106e1d2037780b6f488cdc322ae4d46d6c44a713bde0927455c5338808458fb",
        "5eda901939086eca0e16ce43354f42e6baa1c1c5afdc7a0a9e42e27e87d7aad66013d01767692a90f785190571316000",
        "e8501e3d1e364577a7798b31b061b21166",
    ),
    "b87670516460a6ea2cc450374b6c1cb6",
    concat!(
        "3a12b43c773f49a7d7ac4c1ca469c72aa64c6a0227bba8bb7b86e48cddc9346d1f7a9cb1a56d6bd45a9d4a60c9ff7e17",
        "cea21cf9455e259bcf71ef6722f3b467ff8ad4aebae3b1daf6b37398ad2e3ef37d218869665da41cdad5080b0d994216",
        "8436b9835cb7b66578da1895fe57f751cc26591572716a55fb7532e276d3d81a8bf998aa19c636016829ce7f76a07d20",
        "64759ba59befdfea72d629356611f275ccc232cf61e55219f30c89f0326d17b0fb47779b2e4d29b2d7666772c67cc146",
        "e51b7d8dd2194215bf4d15d7a920183624d2e6711b8e9a5f6b5cba91da1e1553a1c3316501539d4b62a61902de7c15b1",
        "184ab6565ca39f69f8e2353f085456f161ed7eb0b69ad388495edef9cba76f1414ba5ce3174ad600c37931d5cb23597d",
        "8cb2ff7805511a73afd72ce18c9232e65119c4513397996f99564e18d8e139dbde92186c2671da8a5d90aca1166e7d3d",
        "a96c38500a65f747b4e7fcfedbbbff9d2dea542846f3929433126a435d36a19db60062da920723acf1e05ac204b8b984",
        "5600415db11a4a589c0b90a667d4fc2d61e543b22b57061dbd7f1a44cd8fcf31d4189df1213abd9caf7151ef6388f2e0",
        "ddf7f286553c83e51d5681879148195bf7cdc4c29c8eddf7e77e43f43694313bf99786fe6acb8fa765e3f94d48addb1c",
        "bfef1e195dcb4b91c5535a74187a511aba0ad7351dee172ad04bb07dc8be7df61942b6bcba525cc62254c828a80d7f2e",
        "e85bacfb96528f62bebad7e84b1b86eba3919acfdde8ad34df6717fa314ed75a3de7a350b56652498df96bb36a4c10b0",
        "f08b0a9f242d58d6afae60a42dca99aa2c93384110ad6211eae4f28739ed17f90438fb75e638fe4db76794f62f767f96",
        "ae508d4d32da092e39063b8475863f11d3eb6a66b6935396692ed1ecdc857f4c6c7455db83b5675a5abe36dfb231ba46",
        "612f6319b7a91caa2b062a99d2dad7b2296b8ba0508a0eddd24b73e841f1802edc2b6b203673bb2b3c65aa66e13875dc",
        "4bc7ee7130490efa7989b93df07bfdad784668907c08c0a32c59df9b4bb8452e7e449ab49262d1266369b6e028a9e22c",
        "69942903d719b5cf9b5c27ff20cfeadd9b2f1b86c7cae2cf36a328505dacd22500373535f7e8955b53857f93194c8a4c",
        "2f0cc2a80f865302c2a829f14785e0f151e237c693fcf769c320c0d0ed5dcd61daa32ccc676f28acb56f06214c896693",
        "f32092b56f07c6b35a99a66f3376c9667354c84dad8839d83dc1c28995b7b8f43759e595b57469713cc666bd7272ac52",
        "2a7afdaa7f24261da1e6a799940b6dad6765f94b65d738d1aa705c62c7c4559db60d15ed25d2e34b17a1767c0f8fbf81",
        "a7af9ff8199c4adff757e690e4f86659d9681c7502d2a191e807d9555f12aa5d9746c8dfc8ba6c42322fd6f4eb75da0e",
        "aa0febbe4749766cffa056552ae6aef1126bf1617834bc47f1d187ca64b214aa5da1fad1d147c52b851c16a3e6cc4115",
        "d755404cc813fcc795342d60c7f0286fd67b6235edbb8d9c410a9befba8f7a2a2a6cddf2bd41bdce639884e3ac250172",
        "c593d9643139db4f3c3748b78d209bcf61b39d9bc6d02c176b646f8fb8a6cb0964104218fcba9ff16da9d4549f57a2bb",
        "f2ec383361a3fb501da93451ba446b9c31fb86d7ef4d96af3732bd4f1f7471624593a49fd08e6d37854bd4d62a9506da",
        "63866634a8982ec8811618d2d04f2b5084aeefd2f9ffefc99265535960f33bfebad326107646c3a3ce14d69e568d6f53",
        "56708a26cdf68ef885278b0ca2f5105fb459d800000000000000000000000000",
    ),
);
